use {
    super::IteratorTryFlatMapExt,
    crate::{
        compression::{ArchiveFileHandle, ProcessArchive, zip::ZipArchive},
        utils::ExistingPathRead,
    },
    anyhow::{Context, Result},
    case_insensitive_path::{CaseInsensitivePathBuf, ExistingPath, ExistingPathBuf},
    itertools::Itertools,
    parking_lot::Mutex,
    rayon::{iter::ParallelIterator, slice::ParallelSlice},
    std::{collections::BTreeMap, ops::Div, str::FromStr, sync::Arc},
    tap::prelude::*,
    tempfile::TempPath,
    tracing::instrument,
};

#[derive(Clone, derivative::Derivative)]
#[derivative(Debug)]
pub struct WabbajackFileHandle {
    wabbajack_file_path: Arc<ExistingPathBuf>,
    #[derivative(Debug = "ignore")]
    preloaded: Arc<Mutex<BTreeMap<CaseInsensitivePathBuf, TempPath>>>,
}

impl WabbajackFileHandle {
    #[instrument]
    pub fn get_source_data(&self, source_data_id: uuid::Uuid) -> Result<TempPath> {
        let mut preloaded = self.preloaded.lock();
        preloaded
            .remove(&CaseInsensitivePathBuf::from_str(&source_data_id.as_hyphenated().to_string()).context("uuid to be a valid utf8 segment")?)
            .with_context(|| format!("no [{source_data_id:?}] inside wabbajack archive ({:#?})", preloaded.keys().collect_vec()))
    }
    pub(crate) fn from_archive(archive_path: &ExistingPath) -> Result<Self> {
        archive_path
            .open_file_read()
            .and_then(|(at_path, _file)| ZipArchive::new(&at_path).with_context(|| format!("opening archive at path [{at_path:#?}]")))
            .and_then(|mut archive| {
                archive
                    .list_paths()
                    .context("reading archive contents")
                    .and_then(|paths| {
                        drop(archive);
                        let chunk_size = paths.len().div(num_cpus::get()).clamp(1, 64);
                        paths
                            .iter()
                            .collect_vec()
                            .par_chunks(chunk_size)
                            .map(|chunk| {
                                ZipArchive::new(archive_path)
                                    .with_context(|| format!("opening archive at path [{archive_path:#?}]"))
                                    .and_then(|mut archive| {
                                        archive.get_many_handles(chunk).map(|handles| {
                                            handles.into_iter().map(|(path, handle)| {
                                                (match handle {
                                                    ArchiveFileHandle::Zip(named_temp_file) => named_temp_file.into_temp_path(),
                                                    _ => panic!("come on"),
                                                })
                                                .pipe(|temp_path| (path, temp_path))
                                            })
                                        })
                                    })
                            })
                            .collect_vec_list()
                            .into_iter()
                            .flat_map(|chunk| {
                                chunk
                                    .into_iter()
                                    .try_flat_map(|chunk| chunk.into_iter().map(Ok))
                            })
                            .collect::<Result<BTreeMap<_, _>>>()
                            .context("getting all wabbajack archive file handles")
                    })
            })
            .map(Mutex::new)
            .map(Arc::new)
            .map(|preloaded| Self {
                preloaded,
                wabbajack_file_path: Arc::new(archive_path.into_owned()),
            })
    }
}
