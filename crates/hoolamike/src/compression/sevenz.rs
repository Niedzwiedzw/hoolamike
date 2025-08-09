use {
    super::{ProcessArchive, *},
    crate::utils::MaybeWindowsPath,
    std::{collections::HashMap, fs::File, io::BufWriter, ops::Not, path::PathBuf},
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
                files_to_extract
                    .into_iter()
                    .map(|(archive_path, original_file_path)| {
                        let span = info_span!("extracting_file", ?archive_path, ?original_file_path);
                        self.read_file(&original_file_path)
                            .with_context(|| format!("opening [{original_file_path}] ({archive_path:#?})"))
                            .and_then(|file| {
                                file.len().pipe(|expected_size| {
                                    tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
                                        .context("creating temp file")
                                        .and_then(|mut output| {
                                            #[allow(clippy::let_and_return)]
                                            {
                                                let wrote = std::io::copy(
                                                    &mut span.wrap_read(expected_size as _, &mut std::io::Cursor::new(file)),
                                                    &mut BufWriter::new(&mut output),
                                                )
                                                .context("extracting into temp file");
                                                wrote
                                            }
                                            .and_then(|wrote| {
                                                output
                                                    .rewind()
                                                    .context("rewinding output file")
                                                    .and_then(|_| {
                                                        wrote
                                                            .eq(&(expected_size as u64))
                                                            .then_some(output)
                                                            .with_context(|| format!("expected [{expected_size}], found [{wrote}]"))
                                                    })
                                            })
                                        })
                                })
                            })
                            .map(|output| (archive_path, output.pipe(super::ArchiveFileHandle::Zip)))
                    })
                    .collect::<Result<Vec<_>>>()
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
