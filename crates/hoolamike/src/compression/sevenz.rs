use {
    super::{ProcessArchive, *},
    crate::{progress_bars_v2::count_progress_style, utils::MaybeWindowsPath},
    base64::{prelude::BASE64_STANDARD, Engine},
    std::{
        borrow::Cow,
        collections::{BTreeMap, HashMap},
        fs::File,
        io::{BufWriter, Read},
        ops::Not,
        path::PathBuf,
    },
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

// pub type SevenZipFile = ::sevenz_rust2::SevenZReader<File>;
pub type SevenZipArchive = ::sevenz_rust2::ArchiveReader<File>;

#[extension_traits::extension(trait SevenZipArchiveExt)]
impl<R: Read + Seek> ::sevenz_rust2::ArchiveReader<R> {
    fn list_paths_with_originals(&mut self) -> Vec<(String, PathBuf)> {
        self.archive()
            .files
            .iter()
            .filter(|e| e.is_directory.not())
            .map(|e| (e.name.clone(), MaybeWindowsPath(e.name.clone()).into_path()))
            .collect()
    }
}

impl<R: Read + Seek> ProcessArchive for ::sevenz_rust2::ArchiveReader<R> {
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
                        tempfile::Builder::new()
                            .prefix(&entry.name.as_str().pipe(|v| BASE64_STANDARD.encode(v)))
                            .tempfile_in(*crate::consts::TEMP_FILE_DIR)
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
                                        .flush()
                                        .context("flushing")
                                        .and_then(|_| output_file.rewind().context("rewinding output file"))
                                        .and_then(|_| {
                                            wrote
                                                .eq(&expected_size)
                                                .then_some(output_file)
                                                .with_context(|| format!("expected [{expected_size}], found [{wrote}]"))
                                        })
                                })
                            })
                            .with_context(|| format!("when extracting entry {entry:#?}"))
                            .map_err(|e| {
                                let error = Cow::Owned(format!("{e:?}"));
                                sevenz_rust2::Error::Io(std::io::Error::other(e), error)
                            })
                            .map(|out| {
                                output_data.push((original_file_path, super::ArchiveFileHandle::Zip(out)));
                                extracting_files.pb_inc(1);
                                !lookup.is_empty()
                            })
                    }),
                    None => {
                        std::io::copy(reader, &mut std::io::empty())?;
                        std::result::Result::<_, sevenz_rust2::Error>::Ok(!lookup.is_empty())
                    }
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

#[cfg(test)]
mod tests {
    use {
        rand::Rng,
        rayon::iter::{IntoParallelIterator, ParallelIterator},
        tempfile::NamedTempFile,
        tracing::info,
    };

    fn in_tempfile(bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
        NamedTempFile::new()
            .context("creating tempfile")
            .and_then(|mut file| {
                std::io::copy(&mut std::io::Cursor::new(bytes), &mut file)
                    .context("dumpin archive")
                    .and_then(|_| {
                        file.flush()
                            .context("flushing")
                            .and_then(|_| file.rewind().context("rewinding"))
                    })
                    .map(|_| file)
            })
    }

    use super::*;
    #[test_log::test]
    fn test_example_archive_works() -> Result<()> {
        static ARCHIVE: &[u8] = include_bytes!("./example-files/data.7z");
        info!("testing ./example-files/data.7z");

        in_tempfile(ARCHIVE).and_then(|mut file| {
            ::sevenz_rust2::ArchiveReader::new(file.as_file_mut(), "".into())
                .context("opening archive")
                .and_then(|mut archive| {
                    archive.list_paths_with_originals().pipe(|paths| {
                        paths
                            .iter()
                            .map(|(_, path)| path.as_path())
                            .collect::<Vec<_>>()
                            .pipe(|paths| archive.get_many_handles(&paths))
                            .map(|extracted| {
                                extracted
                                    .into_iter()
                                    .for_each(|(path, _)| info!("succesfully extracted [{path:?}]"))
                            })
                    })
                })
        })
    }
    #[test_log::test]
    fn test_weird_1_works() -> Result<()> {
        static ARCHIVE: &[u8] = include_bytes!("./example-files/weird-1.7z");
        info!("testing ./example-files/weird-1.7z");

        in_tempfile(ARCHIVE).and_then(|file| {
            ::sevenz_rust2::ArchiveReader::new(file, "".into())
                .context("opening archive")
                .and_then(|mut archive| {
                    archive.list_paths_with_originals().pipe(|paths| {
                        paths
                            .iter()
                            .map(|(_, path)| path.as_path())
                            .collect::<Vec<_>>()
                            .pipe(|paths| archive.get_many_handles(&paths))
                            .map(|extracted| {
                                extracted
                                    .into_iter()
                                    .for_each(|(path, _)| info!("succesfully extracted [{path:?}]"))
                            })
                    })
                })
        })
    }
    #[test_log::test]
    fn test_weird_1_works_multiple_threads() -> Result<()> {
        static ARCHIVE: &[u8] = include_bytes!("./example-files/weird-1.7z");
        info!("testing ./example-files/weird-1.7z");

        (0..100)
            .into_par_iter()
            .map(|_| {
                in_tempfile(ARCHIVE).and_then(|file| {
                    ::sevenz_rust2::ArchiveReader::new(file, "".into())
                        .context("opening archive")
                        .and_then(|mut archive| {
                            archive.list_paths_with_originals().pipe(|paths| {
                                paths
                                    .iter()
                                    .map(|(_, path)| path.as_path())
                                    .filter(|_| rand::thread_rng().gen::<bool>())
                                    .collect::<Vec<_>>()
                                    .pipe(|paths| archive.get_many_handles(&paths))
                                    .map(|extracted| {
                                        extracted
                                            .into_iter()
                                            .for_each(|(path, _)| info!("succesfully extracted [{path:?}]"))
                                    })
                            })
                        })
                })
            })
            .collect::<Result<Vec<_>>>()
            .map(|_| ())
    }
    #[test_log::test]
    fn test_weird_1_works_multiple_threads_same_file() -> Result<()> {
        static ARCHIVE: &[u8] = include_bytes!("./example-files/weird-1.7z");
        info!("testing ./example-files/weird-1.7z");

        in_tempfile(ARCHIVE).and_then(|file| {
            let path = file.path().to_owned();
            (0..10000)
                .into_par_iter()
                .map(|_| {
                    path.open_file_read().and_then(|(_, file)| {
                        ::sevenz_rust2::ArchiveReader::new(file, "".into())
                            .context("opening archive")
                            .and_then(|mut archive| {
                                archive.list_paths_with_originals().pipe(|paths| {
                                    paths
                                        .iter()
                                        .map(|(_, path)| path.as_path())
                                        .collect::<Vec<_>>()
                                        .pipe(|paths| archive.get_many_handles(&paths))
                                        .map(|extracted| {
                                            extracted
                                                .into_iter()
                                                .filter_map(|f| rand::thread_rng().gen::<bool>().then_some(f))
                                                .for_each(|(path, _)| info!("succesfully extracted [{path:?}]"))
                                        })
                                })
                            })
                    })
                })
                .collect::<Result<Vec<_>>>()
                .map(|_| ())
        })
    }
}
