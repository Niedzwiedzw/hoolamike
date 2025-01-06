use {
    super::{ProcessArchive, *},
    crate::utils::MaybeWindowsPath,
    anyhow::{Context, Result},
    itertools::Itertools,
    std::{collections::HashSet, path::PathBuf},
    tracing::instrument,
};

pub type UnrarFile = tempfile::NamedTempFile;

#[derive(Debug)]
pub struct ArchiveHandle(PathBuf);

impl ArchiveHandle {
    #[tracing::instrument(skip(file))]
    pub fn new(file: &Path) -> Result<Self> {
        unrar::Archive::new(file)
            .open_for_listing()
            .context("could not open archive for listing")
            .and_then(|listing| {
                listing
                    .map(|e| e.context("bad entry"))
                    .map_ok(|_| ())
                    .collect::<Result<()>>()
                    .context("listing archive")
            })
            .map(|_| file.to_owned())
            .map(Self)
            .context("opening archive using unrar")
    }
}
impl ProcessArchive for ArchiveHandle {
    #[instrument(skip(self))]
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        unrar::Archive::new(&self.0)
            .open_for_listing()
            .context("opening for listing")
            .and_then(|opened| {
                opened
                    .map(|f| {
                        f.context("bad file").map(|f| {
                            f.filename
                                .display()
                                .to_string()
                                .pipe(MaybeWindowsPath)
                                .pipe(MaybeWindowsPath::into_path)
                        })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .context("listing archive")
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
                                info_span!("extracting_mutliple_files", file_count=%validated_paths.len()).in_scope(|| {
                                    unrar::Archive::new(&self.0)
                                        .open_for_processing()
                                        .context("opening archive for processing")
                                        .and_then(|iterator| -> Result<_> {
                                            let mut out = vec![];
                                            let mut iterator = Some(iterator);
                                            while let Some(post_header) = iterator
                                                .take()
                                                .context("no iterator")
                                                .and_then(|iterator| iterator.read_header().context("reading header"))?
                                            {
                                                match validated_paths
                                                    .remove(&post_header.entry().filename)
                                                    .then_some(post_header.entry().filename.clone())
                                                {
                                                    None => iterator = Some(post_header.skip().context("skipping entry")?),
                                                    Some(archive_path) => tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
                                                        .context("creating temp file")
                                                        .and_then(|file| {
                                                            file.path()
                                                                .pipe_ref(|temp| {
                                                                    post_header
                                                                        .extract_to(temp)
                                                                        .with_context(|| format!("extracting to [{temp:?}]"))
                                                                })
                                                                .map(|post_extract| {
                                                                    iterator = Some(post_extract);
                                                                    out.push((archive_path, file))
                                                                })
                                                        })?,
                                                }
                                            }
                                            Ok(out)
                                        })
                                        .map(|paths| {
                                            paths
                                                .into_iter()
                                                .map(|(path, file)| (path, self::ArchiveFileHandle::Unrar(file)))
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
        })
    }

    #[instrument(skip(self))]
    fn get_handle<'this>(&mut self, path: &Path) -> Result<super::ArchiveFileHandle> {
        self.get_many_handles(&[path])
            .context("extracting path")
            .and_then(|path| path.into_iter().next().context("no entry in output"))
            .map(|(_, handle)| handle)
    }
}

// impl super::ProcessArchiveFile for UnrarFile {}
