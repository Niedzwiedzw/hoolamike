use {
    super::{ProcessArchive, *},
    crate::{
        compression::case_insensitive_lookup::{case_insensitive_string::CaseInsensitiveString, CaseInsenitiveBasicListing},
        progress_bars_v2::io_progress_style,
        utils::PathFileNameOrEmpty,
    },
    ::compress_tools::*,
    anyhow::{Context, Result},
    itertools::Itertools,
    num::ToPrimitive,
    std::{io::Seek, path::PathBuf},
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

pub type CompressToolsFile = tempfile::NamedTempFile;

#[derive(Debug)]
pub struct ArchiveHandle(std::fs::File);

impl ArchiveHandle {
    pub fn new(mut file: std::fs::File) -> Result<Self> {
        list_archive_files_with_encoding(&mut file, |_| Ok(String::new()))
            .context("listing files")
            .and_then(|_| file.rewind().context("rewinding the stream"))
            .context("could not read with compress-tools (libarchive)")
            .map(|_| Self(file))
    }

    pub fn list_paths_with_originals(&mut self) -> Result<case_insensitive_lookup::CaseInsenitiveBasicListing> {
        self.0.rewind().context("rewinding file").and_then(|_| {
            list_archive_files(&mut self.0)
                .context("listing archive")
                .map(|e| {
                    e.into_iter()
                        .pipe(CaseInsenitiveBasicListing::from_string_paths)
                })
        })
    }
}

impl ProcessArchive for ArchiveHandle {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_paths_with_originals()
            .map(|e| e.keys().map(|k| k.as_path()).collect_vec())
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        self.list_paths_with_originals()
            .and_then(|e| e.plan_extract_lookup(paths))
            .and_then(|mut validated_paths| {
                let _extracting_mutltiple_files = info_span!("extracting_mutliple_files", file_count=%validated_paths.len()).entered();
                compress_tools::ArchiveIteratorBuilder::new(&mut self.0)
                    .filter({
                        cloned![validated_paths];
                        move |e, _| validated_paths.contains_key(&e.into())
                    })
                    .build()
                    .context("building archive iterator")
                    .and_then(|mut iterator| {
                        iterator
                            .try_fold((vec![], info_span!("current_file").entered()), |(mut acc, span), entry| match entry {
                                ArchiveContents::StartOfEntry(entry_path_string, stat) => entry_path_string
                                    .as_str()
                                    .pipe(PathBuf::from)
                                    .pipe(|entry_path| {
                                        drop(span);

                                        validated_paths
                                            .remove(&entry_path.pipe_deref(CaseInsensitiveString::from_path))
                                            .with_context(|| format!("unrequested entry: {entry_path:?}"))
                                            .and_then(|path| {
                                                let temp_file = path
                                                    .requested_path
                                                    .as_ref()
                                                    .named_tempfile_with_context()
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
                                            std::io::copy(&mut span.wrap_read(size.to_u64().context("negative size")?, std::io::Cursor::new(chunk)), acc)
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
                                                    .with_context(|| format!("error extracting {path:?}: expected [{size} bytes], got [{wrote_size} bytes]"))
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
                            .map(|(path, _size, file)| (path.requested_path.as_path(), self::ArchiveFileHandle::CompressTools(file)))
                            .collect_vec()
                    })
                    .and_then(move |finished| {
                        validated_paths
                            .is_empty()
                            .then_some(finished)
                            .with_context(|| format!("not all paths were extracted. missing paths: {validated_paths:#?}"))
                    })
            })
            .with_context(|| {
                format!(
                    "when getting multiple handles out of an archive of kind [{kind:?}]",
                    kind = ArchiveHandleKind::CompressTools
                )
            })
    }
}
