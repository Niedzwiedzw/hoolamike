use {
    super::*,
    crate::{
        install_modlist::directives::create_bsa::tes_4::*, modlist_json::directive::create_bsa_directive::bsa::FileStateData, path::CaseInsensitivePathBuf,
        utils::ExistingPathRead,
    },
    anyhow::{Context, Result},
    ba2::tes4::*,
    case_insensitive_path::Utf8TypedPathToPlatformExt,
    std::str::FromStr,
    tracing::trace,
};

#[instrument(skip(handle_archive, file_states), fields(files=file_states.len()))]
pub fn build_bsa<F: FnOnce(&Archive<'_>, ArchiveOptions, CaseInsensitivePathBuf) -> Result<()>>(
    LazyArchive {
        files: file_states,
        archive_metadata:
            WriteArchiveLocation {
                name: _,
                value,
                archive_type: _,
                archive_flags,
                files_flags,
                archive_compressed,
            },
    }: LazyArchive,
    handle_archive: F,
) -> Result<()> {
    let output_archive_file = CaseInsensitivePathBuf::from_str(&value).context("bad output_archive_file")?;
    let version = Version::FNV;
    let archive_flags = ArchiveFlags::from_bits(archive_flags as _).with_context(|| format!("invalid flags: {archive_flags:b}"))?;
    let archive_types = ArchiveTypes::from_bits(files_flags).with_context(|| format!("invalid file flags: {files_flags:b}"))?;

    let reading_bsa_entries = info_span!("creating_bsa_entries", count=%file_states.len())
        .entered()
        .tap(|pb| {
            pb.pb_set_style(&count_progress_style());
            pb.pb_set_length(file_states.len() as _);
        });
    let preheating_files = info_span!("preheating_files").tap_mut(|pb| {
        pb.pb_set_style(&count_progress_style());
        pb.pb_set_length(file_states.len() as _);
    });
    preheating_files
        .clone()
        .in_scope(|| {
            file_states
                .into_par_iter()
                .map(move |(archive_path, file)| {
                    info_span!("handle_file_state", %archive_path).in_scope(|| {
                        trace!("opening file");
                        file.pipe(|path| path.open_file_read())
                            .and_then(|(path, file)| {
                                LazyArchiveFile::new(
                                    &file,
                                    FileStateData {
                                        flip_compression: true,
                                        index: 0,
                                        path: archive_path.clone(),
                                    },
                                )
                                .with_context(|| format!("loading file at [{path:?}]"))
                            })
                            .and_then(|file| {
                                archive_path
                                    .as_original_path()
                                    .into_windows_encoding_checked()
                                    .and_then(|archive_path| create_key(&archive_path))
                                    .map(|key| (key, file))
                            })
                    })
                })
                .inspect(|_| reading_bsa_entries.pb_inc(1))
                .inspect(|_| preheating_files.pb_inc(1))
                .collect::<Result<Vec<_>>>()
        })
        .and_then(|entries| {
            let building_archive = info_span!("building_archive", path=%output_archive_file).tap(|pb| {
                pb.pb_set_style(&count_progress_style());
                pb.pb_set_length(entries.len() as _);
            });
            building_archive.in_scope(|| {
                entries.pipe_ref(|entries| {
                    entries
                        .par_iter()
                        .map(|(key, file)| {
                            file.as_archive_file(
                                version,
                                match archive_compressed {
                                    true => Some(ba2::CompressionResult::Compressed),
                                    false => Some(ba2::CompressionResult::Decompressed),
                                },
                            )
                            .map(|file| {
                                building_archive.pb_inc(1);
                                (key, file)
                            })
                        })
                        .collect::<Result<Vec<_>>>()
                        .and_then(|entries| {
                            let loading_entries = info_span!("loading_entries").tap_mut(|pb| {
                                pb.pb_set_length(entries.len() as _);
                                pb.pb_set_style(&count_progress_style());
                            });
                            loading_entries
                                .in_scope(|| {
                                    entries
                                        .into_iter()
                                        .inspect(|_| loading_entries.pb_inc(1))
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
                                })
                                .pipe(|archive| {
                                    handle_archive(
                                        &archive,
                                        ArchiveOptions::builder()
                                            .version(version)
                                            .flags(archive_flags)
                                            .types(archive_types)
                                            .build(),
                                        output_archive_file,
                                    )
                                })
                        })
                        .context("creating BSA (skyrim and before) archive")
                })
            })
        })
}
