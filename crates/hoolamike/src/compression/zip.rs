use {
    super::{ProcessArchive, *},
    crate::{
        progress_bars_v2::count_progress_style,
        utils::{AsBase64, MaybeWindowsPath},
    },
    std::{collections::BTreeMap, fs::File, io::BufWriter, path::PathBuf},
    tempfile::NamedTempFile,
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

// pub type ZipArchive = ::zip::read::ZipArchive<File>;

#[derive(Debug)]
pub struct ZipArchive(File);

pub type ZipFile = NamedTempFile;

impl ZipArchive {
    pub fn new(path: &Path) -> Result<Self> {
        path.open_file_read()
            .and_then(|(_path, mut file)| {
                ::zip::ZipArchive::new(&mut file)
                    .context("opening file as zip")
                    .map(drop)
                    .and_then(|_| file.rewind().context("rewinding").map(|_| file))
            })
            .map(Self)
            .and_then(|mut archive| archive.list_paths_with_originals().map(|_| archive))
    }
    fn with_file<T, F: FnOnce(&mut std::fs::File) -> Result<T>>(&mut self, with: F) -> Result<T> {
        self.0
            .pipe_ref_mut(|file| with(file).and_then(|out| file.rewind().context("rewinding file").map(|_| out)))
    }
    fn with_archive<T, F: FnOnce(&mut ::zip::ZipArchive<&mut File>) -> Result<T>>(&mut self, with: F) -> Result<T> {
        self.with_file(|file| {
            ::zip::ZipArchive::new(file)
                .context("reading as archive")
                .and_then(|mut archive| with(&mut archive))
        })
    }
    fn list_paths_with_originals(&mut self) -> Result<Vec<(String, PathBuf)>> {
        self.with_archive(|this| {
            (0..this.len())
                .filter_map(|idx| {
                    this.by_index(idx)
                        .with_context(|| format!("reading file idx [{idx}]"))
                        .map(|file| file.is_file().then_some(file))
                        .transpose()
                        .map(|file| {
                            file.and_then(|file| {
                                file.name().to_string().pipe(|name| {
                                    file.enclosed_name()
                                        .context("file can is not enclosed")
                                        .map(|_| (name.clone(), MaybeWindowsPath(name).into_path()))
                                })
                            })
                        })
                })
                .collect::<Result<_>>()
                .context("listing archive contents")
        })
    }
}

impl ProcessArchive for ZipArchive {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_paths_with_originals()
            .map(|paths| paths.into_iter().map(|(_, p)| p).collect())
    }
    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        self.list_paths_with_originals()
            .map(|paths| {
                paths
                    .into_iter()
                    .map(|(name, path)| (path, name))
                    .collect::<BTreeMap<_, _>>()
            })
            .and_then(|mut name_lookup| {
                paths
                    .iter()
                    .map(|path| {
                        name_lookup
                            .remove(*path)
                            .with_context(|| format!("path [{path:?}] not found in archive:\n{name_lookup:#?}"))
                            .map(|name| ((*path).to_owned(), name))
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("figuring out correct archive paths")
            })
            .and_then(|files_to_extract| {
                let extracting_files = info_span!("extracting_files").tap(|pb| {
                    pb.pb_set_style(&count_progress_style());
                    pb.pb_set_length(files_to_extract.len() as _);
                });

                self.with_archive(|archive| {
                    files_to_extract
                        .into_iter()
                        .map(|(archive_path, file_name)| {
                            let span = info_span!("extracting_file", ?archive_path, ?file_name);

                            archive
                                .by_name(&file_name)
                                .with_context(|| format!("opening [{file_name}] ({archive_path:#?})"))
                                .and_then(|mut file| {
                                    file.size().pipe(|expected_size| {
                                        tempfile::Builder::new()
                                            .prefix(&file_name.to_base64())
                                            .tempfile_in(*crate::consts::TEMP_FILE_DIR)
                                            .context("creating temp file")
                                            .and_then(|mut output| {
                                                #[allow(clippy::let_and_return)]
                                                {
                                                    let wrote = std::io::copy(&mut span.wrap_read(expected_size, &mut file), &mut BufWriter::new(&mut output))
                                                        .context("extracting into temp file");
                                                    wrote
                                                }
                                                .and_then(|wrote| {
                                                    output
                                                        .rewind()
                                                        .context("rewinding output file")
                                                        .and_then(|_| {
                                                            wrote
                                                                .eq(&expected_size)
                                                                .then_some(output)
                                                                .with_context(|| format!("expected [{expected_size}], found [{wrote}]"))
                                                        })
                                                })
                                            })
                                    })
                                })
                                .map(|output| (archive_path, output.pipe(super::ArchiveFileHandle::Zip)))
                                .tap_ok(|_| {
                                    extracting_files.pb_inc(1);
                                })
                        })
                        .collect::<Result<Vec<_>>>()
                })
            })
            .with_context(|| {
                format!(
                    "when getting multiple handles out of an archive of kind [{kind:?}]",
                    kind = ArchiveHandleKind::Zip
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
