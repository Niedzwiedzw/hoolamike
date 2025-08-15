use {
    super::{ProcessArchive, *},
    crate::{
        install_modlist::directives::IteratorTryFlatMapExt,
        progress_bars_v2::count_progress_style,
        utils::{AsBase64, MaybeWindowsPath, PathFileNameOrEmpty},
    },
    itertools::Itertools,
    sevenz_rust2::{BlockDecoder, Password},
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

pub struct SevenZipArchive {
    file: File,
    archive: ::sevenz_rust2::Archive,
}

impl SevenZipArchive {
    pub fn new(mut file: File) -> Result<Self> {
        ::sevenz_rust2::Archive::read(&mut file, no_password())
            .context("reading archive contents")
            .and_then(|archive| {
                file.rewind()
                    .context("rewinding file")
                    .map(|_| Self { file, archive })
            })
    }
}

thread_local! {
    static NO_PASSWORD: &'static Password = Password::from("").pipe(Box::new).pipe(Box::leak);
}

fn no_password() -> &'static Password {
    NO_PASSWORD.with(|p| *p)
}

impl<R: Read + Seek> SevenZipArchiveExt for ::sevenz_rust2::ArchiveReader<R> {
    fn list_paths_with_originals(&self) -> Vec<(String, PathBuf, Option<usize>)> {
        self.archive().list_paths_with_originals()
    }
}

impl SevenZipArchiveExt for SevenZipArchive {
    fn list_paths_with_originals(&self) -> Vec<(String, PathBuf, Option<usize>)> {
        self.archive.list_paths_with_originals()
    }
}

#[extension_traits::extension(trait SevenZipArchiveExt)]
impl ::sevenz_rust2::Archive {
    fn list_paths_with_originals(&self) -> Vec<(String, PathBuf, Option<usize>)> {
        self.files
            .iter()
            .zip(self.stream_map.file_block_index.iter())
            .filter(|(e, _block_index)| e.is_directory.not())
            .map(|(e, block_index)| (e.name.clone(), MaybeWindowsPath(e.name.clone()).into_path(), *block_index))
            .collect()
    }
}

impl ProcessArchive for SevenZipArchive {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.archive
            .list_paths_with_originals()
            .pipe(|paths| paths.into_iter().map(|(_, p, _)| p).collect::<Vec<_>>())
            .pipe(Ok)
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        self.archive
            .list_paths_with_originals()
            .pipe(|paths| {
                paths
                    .into_iter()
                    .map(|(name, path, block_index)| (path, (name, block_index)))
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
                                    .tap_ok(|name| warn!("found case-insensitive name: [{name:?}]"))
                            })
                            .map(|name| ((*path).to_owned(), name))
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("figuring out correct archive paths")
            })
            .and_then(|files_to_extract| {
                let lookup = files_to_extract
                    .into_iter()
                    .map(|(k, (v, idx))| (v, (k, idx)))
                    .collect::<BTreeMap<_, _>>();
                let extracting_files = info_span!("extracting_files").tap(|pb| {
                    pb.pb_set_style(&count_progress_style());
                    pb.pb_set_length(lookup.len() as _);
                });
                lookup
                    .into_iter()
                    .map(|(k, (v, idx))| {
                        idx.with_context(|| format!("[{k}] (requested path {v:?}) has no index!"))
                            .map(|idx| (k, (v, idx)))
                    })
                    .collect::<Result<Vec<_>>>()
                    .and_then(|paths| {
                        paths
                            .into_iter()
                            .sorted_unstable_by_key(|(_, (_, idx))| *idx)
                            .chunk_by(|(_k, (_v, idx))| *idx)
                            .into_iter()
                            .map(|(block_idx, chunk)| {
                                (
                                    block_idx,
                                    chunk
                                        .into_iter()
                                        .map(|(k, (v, _))| (k, v))
                                        .collect::<BTreeMap<_, _>>(),
                                )
                            })
                            .collect_vec()
                            .into_iter()
                            .map(|(block_idx, mut lookup)| {
                                let mut output_data = Vec::with_capacity(lookup.len());
                                let block = BlockDecoder::new(1, block_idx, &self.archive, no_password(), &mut self.file);

                                block
                                    .for_each_entries(&mut |entry, reader| match lookup.remove(&entry.name) {
                                        Some(original_file_path) => entry.size().pipe(|expected_size| {
                                            let span = info_span!("extracting_file", archive_path=%entry.name, ?original_file_path);
                                            original_file_path
                                                .named_tempfile_with_context()
                                                .and_then(|mut output_file| {
                                                    #[allow(clippy::let_and_return)]
                                                    {
                                                        let result = std::io::copy(
                                                            &mut span.wrap_read(expected_size as _, reader),
                                                            &mut BufWriter::new(&mut output_file),
                                                        )
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
                                    .with_context(|| format!("decoding chunk from [{block_idx}]"))
                                    .map(|_| output_data)
                                    .and_then(|data| match lookup.is_empty() {
                                        true => Ok(data),
                                        false => Err(anyhow::anyhow!(
                                            "not all entries were extracted:\nextracted:\n{}\n\nremaining entries:\n{lookup:#?}",
                                            data.len()
                                        )),
                                    })
                            })
                            .try_flat_map(|v| v.into_iter().map(Ok))
                            .collect::<Result<Vec<_>>>()
                    })
                    .context("extracting multiple paths failed")
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

    fn in_tempfile<T>(bytes: &[u8], in_tempfile: impl FnOnce(File, &Path) -> Result<T>) -> Result<T> {
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
                    .and_then(|_| file.keep().context("keeping file"))
                    .and_then(|(file, path)| in_tempfile(file, &path).and_then(|val| std::fs::remove_file(&path).context("removing").map(|_| val)))
            })
    }

    use super::*;
    #[test_log::test]
    fn test_example_archive_works() -> Result<()> {
        static ARCHIVE: &[u8] = include_bytes!("./example-files/data.7z");
        info!("testing ./example-files/data.7z");

        in_tempfile(ARCHIVE, |file, _| {
            SevenZipArchive::new(file)
                .context("opening archive")
                .and_then(|mut archive| {
                    archive.list_paths_with_originals().pipe(|paths| {
                        paths
                            .iter()
                            .map(|(_, path, _)| path.as_path())
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

        in_tempfile(ARCHIVE, |file, _| {
            SevenZipArchive::new(file)
                .context("opening archive")
                .and_then(|mut archive| {
                    archive.list_paths_with_originals().pipe(|paths| {
                        paths
                            .iter()
                            .map(|(_, path, _)| path.as_path())
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
                in_tempfile(ARCHIVE, |file, _| {
                    SevenZipArchive::new(file)
                        .context("opening archive")
                        .and_then(|mut archive| {
                            archive.list_paths_with_originals().pipe(|paths| {
                                paths
                                    .iter()
                                    .map(|(_, path, _)| path.as_path())
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

        in_tempfile(ARCHIVE, |_file, path| {
            let path = path.to_owned();
            (0..10000)
                .into_par_iter()
                .map(|_| {
                    path.open_file_read().and_then(|(_, file)| {
                        SevenZipArchive::new(file)
                            .context("opening archive")
                            .and_then(|mut archive| {
                                archive.list_paths_with_originals().pipe(|paths| {
                                    paths
                                        .iter()
                                        .map(|(_, path, _)| path.as_path())
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
