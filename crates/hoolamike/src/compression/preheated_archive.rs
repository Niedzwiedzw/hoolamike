use {
    super::{ProcessArchive, SeekWithTempFileExt},
    crate::compression::ArchiveHandle,
    anyhow::{Context, Result},
    itertools::Itertools,
    rayon::iter::{IntoParallelRefIterator, ParallelIterator},
    std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    },
    tap::prelude::*,
    tempfile::TempPath,
};
#[derive(Debug)]
pub struct PreheatedArchive {
    pub paths: BTreeMap<PathBuf, TempPath>,
}

impl PreheatedArchive {
    pub fn from_archive_concurrent(archive: &Path, chunk_size: usize) -> Result<Self> {
        ArchiveHandle::with_guessed(archive, archive.extension(), |mut a| a.list_paths())
            .and_then(|paths| {
                paths
                    .chunks(chunk_size)
                    .collect_vec()
                    .par_iter()
                    .copied()
                    .map(move |chunk| {
                        ArchiveHandle::with_guessed(archive, archive.extension(), |mut archive| {
                            archive
                                .get_many_handles(chunk.iter().map(|p| p.as_path()).collect_vec().as_slice())
                                .context("getting many handles")
                        })
                        .and_then(|handles| {
                            handles
                                .into_iter()
                                .map(|(path, handle)| {
                                    handle
                                        .seek_with_temp_file_blocking_raw(0)
                                        .context("preheating file")
                                        .map(|(_, handle)| (path, handle))
                                })
                                .collect::<Result<BTreeMap<_, _>>>()
                                .context("some files could not be preheated")
                        })
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("some chunks failed")
                    .map(|chunks| {
                        chunks
                            .into_iter()
                            .fold(BTreeMap::new(), |acc, next| acc.tap_mut(|acc| acc.extend(next)))
                    })
                    .map(|paths| Self { paths })
            })
            .with_context(|| format!("preheating [{archive:?}]"))
    }
}
