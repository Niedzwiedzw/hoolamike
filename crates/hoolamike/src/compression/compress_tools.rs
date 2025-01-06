use {
    super::{ProcessArchive, *},
    crate::{progress_bars_v2::io_progress_style, utils::MaybeWindowsPath},
    ::compress_tools::*,
    anyhow::{Context, Result},
    itertools::Itertools,
    num::ToPrimitive,
    std::{
        collections::HashSet,
        io::{BufWriter, Seek},
        path::PathBuf,
    },
    tracing::{instrument, trace, trace_span},
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

pub type CompressToolsFile = tempfile::NamedTempFile;

#[derive(Debug)]
pub struct ArchiveHandle(std::fs::File);

impl ArchiveHandle {
    #[tracing::instrument(skip(file))]
    pub fn new(mut file: std::fs::File) -> Result<Self> {
        list_archive_files_with_encoding(&mut file, |_| Ok(String::new()))
            .context("listing files")
            .and_then(|_| file.rewind().context("rewinding the stream"))
            .context("could not read with compress-tools (libarchive)")
            .map(|_| Self(file))
    }

    #[tracing::instrument(skip(self))]
    pub fn get_handle(&mut self, for_path: &Path) -> Result<CompressToolsFile> {
        self.0.rewind().context("rewinding file")?;
        let lookup = for_path.display().to_string();
        list_archive_files(&mut self.0)
            .context("listing archive")
            .map(|files| files.into_iter().collect::<std::collections::HashSet<_>>())
            .and_then(|files| {
                files
                    .contains(&lookup)
                    .then_some(&lookup)
                    .with_context(|| format!("no [{lookup}] in {files:?}"))
                    .tap_ok(|lookup| trace!("[{lookup}] found in [{files:?}]"))
            })
            .and_then(|lookup| {
                self.0.rewind().context("rewinding file")?;
                tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
                    .context("creating temporary file for output")
                    .and_then(|mut temp_file| {
                        {
                            let mut writer = BufWriter::new(&mut temp_file);
                            trace_span!("uncompress_archive_file")
                                .in_scope(|| uncompress_archive_file(&mut tracing::Span::current().wrap_read(0, &mut self.0), &mut writer, lookup))
                        }
                        .context("extracting archive")
                        .tap_ok(|bytes| trace!(%bytes, "extracted from CompressTools archive"))
                        .and_then(|_| {
                            temp_file
                                .flush()
                                .and_then(|_| temp_file.rewind())
                                .context("rewinding to beginning of file")
                                .map(|_| temp_file)
                        })
                    })
            })
    }
}

impl ProcessArchive for ArchiveHandle {
    #[instrument(skip(self))]
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        ::compress_tools::list_archive_files(&mut self.0)
            .context("listing archive files")
            .map(|e| {
                e.into_iter()
                    .map(MaybeWindowsPath)
                    .map(MaybeWindowsPath::into_path)
                    .collect()
            })
            .and_then(|out| self.0.rewind().context("rewinding file").map(|_| out))
    }

    #[instrument(skip(self))]
    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        info_span!("getting_many_handles_compress_tools").in_scope(|| {
            self.list_paths().and_then(|listed| {
                listed
                    .into_iter()
                    .collect::<HashSet<_>>()
                    .pipe(|mut listed| {
                        paths
                            .iter()
                            .map(|expected| {
                                listed
                                    .remove(*expected)
                                    .then(|| expected.to_owned().pipe(|v| v.to_owned()))
                                    .with_context(|| format!("path {expected:?} not found in {listed:#?}"))
                            })
                            .collect::<Result<HashSet<PathBuf>>>()
                            .context("some paths were not found")
                            .and_then(|mut validated_paths| {
                                let _extracting_mutltiple_files = info_span!("extracting_mutliple_files", file_count=%validated_paths.len()).entered();
                                compress_tools::ArchiveIteratorBuilder::new(&mut self.0)
                                    .filter({
                                        cloned![validated_paths];
                                        move |e, _| validated_paths.contains(Path::new(e))
                                    })
                                    .build()
                                    .context("building archive iterator")
                                    .and_then(|mut iterator| {
                                        iterator
                                            .try_fold((vec![], info_span!("current_file").entered()), |(mut acc, span), entry| match entry {
                                                ArchiveContents::StartOfEntry(entry_path, stat) => entry_path.pipe(PathBuf::from).pipe(|entry_path| {
                                                    drop(span);

                                                    validated_paths
                                                        .remove(entry_path.as_path())
                                                        .then_some(entry_path.clone())
                                                        .with_context(|| format!("unrequested entry: {entry_path:?}"))
                                                        .and_then(|path| {
                                                            let temp_file = tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
                                                                .context("creating a temp file for output")?;
                                                            Ok((
                                                                acc.tap_mut(|acc| acc.push((path, stat.st_size, temp_file))),
                                                                info_span!("current_file", entry_path=%entry_path.display())
                                                                    .tap_mut(|pb| {
                                                                        pb.pb_set_length(stat.st_size as u64);
                                                                        pb.pb_set_style(&io_progress_style());
                                                                    })
                                                                    .entered(),
                                                            ))
                                                        })
                                                }),
                                                ArchiveContents::DataChunk(chunk) => acc
                                                    .last_mut()
                                                    .context("no write in progress")
                                                    .and_then({
                                                        cloned![span];
                                                        |(_, size, acc)| {
                                                            std::io::copy(
                                                                &mut span.wrap_read(size.to_u64().context("negative size")?, std::io::Cursor::new(chunk)),
                                                                acc,
                                                            )
                                                            .context("writing to temp file failed")
                                                        }
                                                    })
                                                    .map(|_| (acc, span)),
                                                ArchiveContents::EndOfEntry => acc
                                                    .last_mut()
                                                    .context("finished entry before reading anything")
                                                    .and_then(|(path, size, temp_file)| {
                                                        temp_file
                                                            .stream_len()
                                                            .context("reading size")
                                                            .and_then(|wrote_size| {
                                                                ((*size) as u64)
                                                                    .eq(&wrote_size)
                                                                    .then_some(())
                                                                    .with_context(|| {
                                                                        format!("error extracting {path:?}: expected [{size} bytes], got [{wrote_size} bytes]")
                                                                    })
                                                                    .map(|_| temp_file)
                                                            })
                                                            .and_then(|temp_file| {
                                                                temp_file
                                                                    .flush()
                                                                    .and_then(|_| temp_file.rewind())
                                                                    .context("rewinding to beginning of file")
                                                                    .map(|_| temp_file)
                                                            })
                                                            .map(drop)
                                                    })
                                                    .map(|_| (acc, span)),
                                                ArchiveContents::Err(error) => Err(error).with_context(|| {
                                                    format!(
                                                        "when reading: {}",
                                                        acc.last_mut()
                                                            .map(|(path, size, _)| format!("{path:?} size={size}"))
                                                            .unwrap_or_else(|| "before reading started".to_string()),
                                                    )
                                                }),
                                            })
                                            .context("reading multiple paths from archive")
                                    })
                                    .map(|(paths, _span)| paths)
                                    .map(|paths| {
                                        paths
                                            .into_iter()
                                            .map(|(path, _size, file)| (path, self::ArchiveFileHandle::CompressTools(file)))
                                            .collect_vec()
                                    })
                                    .and_then(move |finished| {
                                        validated_paths
                                            .is_empty()
                                            .then_some(finished)
                                            .with_context(|| format!("not all paths were extracted. missing paths: {validated_paths:#?}"))
                                    })
                            })
                    })
            })
        })
    }

    #[instrument(skip(self))]
    fn get_handle<'this>(&mut self, path: &Path) -> Result<super::ArchiveFileHandle> {
        self.get_handle(path)
            .map(super::ArchiveFileHandle::CompressTools)
    }
}

impl super::ProcessArchiveFile for CompressToolsFile {}
