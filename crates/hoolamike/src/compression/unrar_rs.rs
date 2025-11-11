use {
    super::{ProcessArchive, *},
    crate::{
        compression::case_insensitive_lookup::CaseInsenitiveBasicListing,
        path::{Path, PathBuf},
        utils::PathFileNameOrEmpty,
    },
    anyhow::{Context, Result},
    itertools::Itertools,
};

pub type UnrarFile = tempfile::NamedTempFile;

#[derive(Debug)]
pub struct ArchiveHandle(ExistingPathBuf);

impl ArchiveHandle {
    pub fn new(file: &ExistingPath) -> Result<Self> {
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

impl ArchiveHandle {
    fn list_paths_with_originals(&self) -> Result<CaseInsenitiveBasicListing> {
        unrar::Archive::new(&self.0)
            .open_for_listing()
            .context("opening for listing")
            .and_then(|opened| {
                opened
                    .filter_ok(|f| f.is_file())
                    .map(|f| f.context("bad file").map(|f| f.filename.clone()))
                    .process_results(|files| CaseInsenitiveBasicListing::from_paths(files))
                    .flatten()
            })
    }
}

impl ProcessArchive for ArchiveHandle {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_paths_with_originals()
            .map(|l| l.into_iter().map(|(p, _)| p).collect_vec())
            .context("listing archive")
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        self.list_paths_with_originals()
            .and_then(|listing| listing.plan_extract_lookup(paths))
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
                                    .remove_entry(
                                        &post_header
                                            .entry()
                                            .filename
                                            .pipe_deref(PathBuf::from_path)
                                            .context("bad entry")?,
                                    )
                                    .map(|(e, _)| e)
                                {
                                    None => iterator = Some(post_header.skip().context("skipping entry")?),
                                    Some(archive_path) => archive_path
                                        .as_path()
                                        .named_tempfile_with_context()
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
            .with_context(|| {
                format!(
                    "when getting multiple handles out of an archive of kind [{kind:?}]",
                    kind = ArchiveHandleKind::Unrar
                )
            })
    }
}

// impl super::ProcessArchiveFile for UnrarFile {}
