use {
    super::{ProcessArchive, *},
    crate::{progress_bars_v2::count_progress_style, utils::MaybeWindowsPath},
    std::{
        borrow::Cow,
        collections::{BTreeMap, HashMap},
        fs::File,
        io::BufWriter,
        ops::Not,
        path::PathBuf,
    },
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

// pub type SevenZipFile = ::sevenz_rust2::SevenZReader<File>;
pub type SevenZipArchive = ::sevenz_rust2::ArchiveReader<File>;

#[extension_traits::extension(trait SevenZipArchiveExt)]
impl SevenZipArchive {
    fn list_paths_with_originals(&mut self) -> Vec<(String, PathBuf)> {
        self.archive()
            .files
            .iter()
            .filter(|e| e.is_directory.not())
            .map(|e| (e.name.clone(), MaybeWindowsPath(e.name.clone()).into_path()))
            .collect()
    }
}

impl ProcessArchive for SevenZipArchive {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_paths_with_originals()
            .pipe(|paths| paths.into_iter().map(|(_, p)| p).collect::<Vec<_>>())
            .pipe(Ok)
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        self.set_thread_count(1);
        self.list_paths_with_originals()
            .pipe(|paths| {
                paths
                    .into_iter()
                    .map(|(name, path)| (path, name))
                    .collect::<HashMap<_, _>>()
            })
            .pipe(|mut name_lookup| {
                paths
                    .iter()
                    .map(|path| {
                        name_lookup
                            .remove(*path)
                            .with_context(|| format!("path [{path:?}] not found in archive:\n{name_lookup:#?}"))
                            .or_else(|reason| {
                                name_lookup
                                    .iter()
                                    .find_map(|(key, name)| {
                                        key.to_string_lossy()
                                            .to_lowercase()
                                            .eq(&path.to_string_lossy().to_lowercase())
                                            .then_some(name.clone())
                                    })
                                    .context("could not even find a case-insensitive path")
                                    .with_context(|| format!("tried because:\n{reason:?}"))
                                    .tap_ok(|name| warn!("found case-insensitive name: [{name}]"))
                            })
                            .map(|name| ((*path).to_owned(), name))
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("figuring out correct archive paths")
            })
            .and_then(|files_to_extract| {
                let mut lookup = files_to_extract
                    .into_iter()
                    .map(|(k, v)| (v, k))
                    .collect::<BTreeMap<_, _>>();
                let mut output_data = Vec::with_capacity(lookup.len());
                let extracting_files = info_span!("extracting_files").tap(|pb| {
                    pb.pb_set_style(&count_progress_style());
                    pb.pb_set_length(lookup.len() as _);
                });
                self.for_each_entries(|entry, reader| match lookup.remove(&entry.name) {
                    Some(original_file_path) => entry.size().pipe(|expected_size| {
                        let span = info_span!("extracting_file", archive_path=%entry.name, ?original_file_path);
                        tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
                            .context("creating temp file")
                            .and_then(|mut output_file| {
                                #[allow(clippy::let_and_return)]
                                {
                                    let result = std::io::copy(&mut span.wrap_read(expected_size as _, reader), &mut BufWriter::new(&mut output_file))
                                        .context("extracting into temp file");
                                    result
                                }
                                .and_then(|wrote| {
                                    output_file
                                        .rewind()
                                        .context("rewinding output file")
                                        .and_then(|_| {
                                            wrote
                                                .eq(&expected_size)
                                                .then_some(output_file)
                                                .with_context(|| format!("expected [{expected_size}], found [{wrote}]"))
                                        })
                                })
                            })
                            .map_err(|e| sevenz_rust2::Error::Io(std::io::Error::other(e), Cow::Borrowed("something went wrong when extracting")))
                            .map(|out| {
                                output_data.push((original_file_path, super::ArchiveFileHandle::Zip(out)));
                                extracting_files.pb_inc(1);
                                !lookup.is_empty()
                            })
                    }),
                    None => std::result::Result::<_, sevenz_rust2::Error>::Ok(!lookup.is_empty()),
                })
                .map(|_| output_data)
                .context("extracting multiple paths failed")
                .and_then(|data| match lookup.is_empty() {
                    true => Ok(data),
                    false => Err(anyhow::anyhow!(
                        "not all entries were extracted:\nextracted:\n{}\n\nremaining entries:\n{lookup:#?}",
                        data.len()
                    )),
                })
            })
            .with_context(|| {
                format!(
                    "when getting multiple handles out of an archive of kind [{kind:?}]",
                    kind = ArchiveHandleKind::SevenzRust2
                )
            })
    }
    fn get_handle(&mut self, path: &Path) -> Result<super::ArchiveFileHandle> {
        self.get_many_handles(&[path])
            .context("getting file handles")
            .and_then(|output| output.into_iter().next().context("no output"))
            .map(|(_, file)| file)
    }
}
