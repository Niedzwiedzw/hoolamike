use {
    super::count_progress_style,
    crate::{
        modlist_json::{
            directive::create_bsa_directive::bsa::{self, Bsa, DirectiveStateData, FileStateData},
            type_guard::WithTypeGuard,
        },
        utils::ExistingPathRead,
    },
    anyhow::{Context, Result},
    ba2::{
        Borrowed,
        CompressionResult,
        ReaderWithOptions,
        tes4::{Archive, ArchiveFlags, ArchiveKey, ArchiveOptions, ArchiveTypes, Directory, DirectoryKey, File, FileReadOptions, Version},
    },
    case_insensitive_path::{CaseInsensitivePathBuf, ExistingPath, Utf8TypedPathToPlatformExt},
    rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator},
    tap::prelude::*,
    tracing::{debug, info_span, instrument},
    tracing_indicatif::span_ext::IndicatifSpanExt,
    typed_path::Utf8WindowsPath,
};

#[derive(Debug)]
pub struct LazyArchiveFile<Directive> {
    file: memmap2::Mmap,
    directive: Directive,
}

impl<Directive: std::fmt::Debug> LazyArchiveFile<Directive> {
    #[instrument]
    pub fn new(from_file: &std::fs::File, directive: Directive) -> Result<Self> {
        // SAFETY: do not touch that file while it's opened please
        debug!("creating file handle");
        unsafe { memmap2::Mmap::map(from_file) }
            .context("creating file")
            .tap_ok(super::try_optimize_memory_mapping)
            .map(|file| Self { file, directive })
    }
    fn as_bytes(&self) -> &[u8] {
        &self.file[..]
    }
}

impl LazyArchiveFile<FileStateData> {
    #[instrument]
    pub fn as_archive_file(&self, version: Version, compression_result: Option<CompressionResult>) -> Result<File<'_>> {
        self.directive.pipe_ref(
            |FileStateData {
                 flip_compression: _,
                 index: _,
                 path: _,
             }| {
                File::read(
                    Borrowed(self.as_bytes()),
                    &FileReadOptions::builder()
                        .version(version)
                        .compression_result(compression_result.unwrap_or(CompressionResult::Compressed))
                        .build(),
                )
                .context("reading file using memory mapping")
                .context("building bsa archive file")
                .tap_ok(|file| tracing::debug!(size=%file.len(), "loaded file"))
            },
        )
    }
}

#[instrument]
pub fn create_key<'a>(path: &Utf8WindowsPath) -> Result<(ArchiveKey<'a>, DirectoryKey<'a>)> {
    path.file_name()
        .context("path has no file name at the end")
        .and_then(|directory_key| {
            path.parent()
                .context("cannot insert files at root, right?")
                .map(|archive_key| {
                    (
                        archive_key.pipe(|path| {
                            path.tap(|path| {
                                tracing::debug!("deriving archive key  for {path:?}");
                            })
                            .as_str()
                            .conv::<ArchiveKey>()
                        }),
                        directory_key
                            .tap(|directory_key| {
                                tracing::debug!("deriving direcotry key for {directory_key:?}");
                            })
                            .conv::<DirectoryKey>(),
                    )
                })
        })
        .with_context(|| format!("reading archive key and directory key for `{path}`"))
}

#[instrument(skip(handle_archive, file_states))]
pub fn create_archive<F: FnOnce(&Archive<'_>, ArchiveOptions, CaseInsensitivePathBuf) -> Result<()>>(
    temp_bsa_dir: &ExistingPath,
    Bsa {
        hash: _,
        size: _,
        to,
        temp_id,
        file_states,
        state:
            WithTypeGuard {
                inner:
                    DirectiveStateData {
                        archive_flags,
                        file_flags,
                        magic: _,
                        version,
                    },
                ..
            },
    }: Bsa,
    handle_archive: F,
) -> Result<()> {
    let version = match version {
        103 => Version::v103,
        104 => Version::v104,
        105 => Version::v105,
        other => anyhow::bail!("unsuppored version: {other}"),
    };
    let archive_flags = ArchiveFlags::from_bits(archive_flags).with_context(|| format!("invalid flags: {archive_flags:b}"))?;
    let archive_types = {
        let file_flags = match file_flags {
            bsa::Either::Left(normal) => normal,
            bsa::Either::Right(weird) => {
                tracing::warn!("encountered a weird file_flags: should be 16 bit but got 32 bit. casting and hoping for the best ({weird:b})");
                weird as u16
            }
        };
        ArchiveTypes::from_bits(file_flags).with_context(|| format!("invalid file flags: {file_flags:b}"))?
    };

    let temp_id_dir = temp_bsa_dir
        .join_checked(&temp_id)
        .map(|temp_id| temp_id.case_insensitive())
        .context("validaing temp id dir")?;
    let reading_bsa_entries = info_span!("creating_bsa_entries", count=%file_states.len())
        .entered()
        .tap(|pb| {
            pb.pb_set_style(&count_progress_style());
            pb.pb_set_length(file_states.len() as _);
        });
    file_states
        .into_par_iter()
        .map(move |WithTypeGuard { inner: file_state_data, .. }| {
            info_span!("handle_file_state", ?file_state_data).in_scope(|| {
                temp_id_dir
                    .join_case_insensitive(file_state_data.path.clone())
                    .and_then(|path| path.try_exists())
                    .and_then(|path| path.open_file_read())
                    .and_then(|(path, file)| LazyArchiveFile::new(&file, file_state_data.clone()).with_context(|| format!("loading file at [{path:?}]")))
                    .and_then(|file| {
                        file_state_data
                            .path
                            .as_original_path()
                            .into_windows_encoding_checked()
                            .and_then(|path| create_key(path.as_path()))
                            .map(|key| (key, file))
                    })
            })
        })
        .inspect(|_| reading_bsa_entries.pb_inc(1))
        .collect::<Result<Vec<_>>>()
        .and_then(|entries| {
            let building_archive = info_span!("building_archive").entered().tap(|pb| {
                pb.pb_set_style(&count_progress_style());
                pb.pb_set_length(entries.len() as _);
            });
            entries.pipe_ref(|entries| {
                entries
                    .par_iter()
                    .map(|(key, file)| {
                        file.as_archive_file(version, None).map(|file| {
                            building_archive.pb_inc(1);
                            (key, file)
                        })
                    })
                    .collect::<Result<Vec<_>>>()
                    .and_then(|entries| {
                        entries
                            .into_iter()
                            .fold(Archive::new(), |acc, ((archive_key, directory_key), file)| {
                                acc.tap_mut(|acc| match acc.get_mut(archive_key) {
                                    Some(directory) => {
                                        directory.insert(directory_key.clone(), file);
                                    }
                                    None => {
                                        acc.insert(
                                            archive_key.clone(),
                                            Directory::default().tap_mut(|directory| {
                                                directory.insert(directory_key.clone(), file);
                                            }),
                                        );
                                    }
                                })
                            })
                            .pipe(|archive| {
                                handle_archive(
                                    &archive,
                                    ArchiveOptions::builder()
                                        .version(version)
                                        .flags(archive_flags)
                                        .types(archive_types)
                                        .build(),
                                    to,
                                )
                            })
                    })
                    .context("creating BSA (skyrim and before) archive")
            })
        })
}
