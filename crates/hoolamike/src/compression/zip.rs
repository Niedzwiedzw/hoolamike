use {
    super::{ProcessArchive, *},
    crate::{
        compression::case_insensitive_lookup::{CaseInsenitiveBasicListing, Entry},
        progress_bars_v2::count_progress_style,
        utils::PathFileNameOrEmpty,
    },
    itertools::Itertools,
    std::{fs::File, io::BufWriter, path::PathBuf},
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
    fn list_paths_with_originals(&mut self) -> Result<CaseInsenitiveBasicListing> {
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
                                        .map(|_| name.clone())
                                })
                            })
                        })
                })
                .process_results(|e| CaseInsenitiveBasicListing::from_string_paths(e.into_iter()))
                .context("listing archive contents")
        })
    }
}

impl ProcessArchive for ZipArchive {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_paths_with_originals().map(|paths| {
            paths
                .into_iter()
                .map(|(path, ())| path.original.pipe_deref(PathBuf::from))
                .collect()
        })
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        self.list_paths_with_originals()
            .and_then(|name_lookup| name_lookup.plan_extract_list(paths))
            .and_then(|files_to_extract| {
                let extracting_files = info_span!("extracting_files").tap(|pb| {
                    pb.pb_set_style(&count_progress_style());
                    pb.pb_set_length(files_to_extract.len() as _);
                });

                self.with_archive(|archive| {
                    files_to_extract
                        .into_iter()
                        .map(
                            |Entry {
                                 archive_path,
                                 requested_path,
                                 extra_value: (),
                             }| {
                                let span = info_span!("extracting_file", ?archive_path, ?requested_path);

                                archive
                                    .by_name(archive_path.as_ref())
                                    .with_context(|| format!("opening [{requested_path:?}] ({archive_path})"))
                                    .and_then(|mut file| {
                                        file.size().pipe(|expected_size| {
                                            archive_path
                                                .as_ref()
                                                .named_tempfile_with_context()
                                                .and_then(|mut output| {
                                                    #[allow(clippy::let_and_return)]
                                                    {
                                                        let wrote =
                                                            std::io::copy(&mut span.wrap_read(expected_size, &mut file), &mut BufWriter::new(&mut output))
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
                                    .map(|output| (requested_path.as_path(), output.pipe(super::ArchiveFileHandle::Zip)))
                                    .tap_ok(|_| {
                                        extracting_files.pb_inc(1);
                                    })
                            },
                        )
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
