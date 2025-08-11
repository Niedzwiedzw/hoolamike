use {
    super::queued_archive_task::SourceKind,
    crate::{
        compression::{ArchiveHandleKind, ProcessArchive, SeekWithTempFileExt},
        install_modlist::directives::IteratorTryFlatMapExt,
        progress_bars_v2::count_progress_style,
    },
    anyhow::{Context, Result},
    indexmap::IndexMap,
    itertools::Itertools,
    nonempty::NonEmpty,
    rand::seq::SliceRandom,
    rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator},
    std::{
        collections::{BTreeMap, BTreeSet},
        iter::once,
        path::PathBuf,
        sync::Arc,
    },
    tap::prelude::*,
    tempfile::TempPath,
    tracing::info_span,
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

type PreheatedArchiveHashPathsInner = BTreeMap<NonEmpty<PathBuf>, Arc<SourceKind>>;

pub struct PreheatedArchiveHashPaths(PreheatedArchiveHashPathsInner);

impl PreheatedArchiveHashPaths {
    pub fn get_archive(&self, path: NonEmpty<PathBuf>) -> Result<Arc<SourceKind>> {
        match path.len() {
            1 => Ok(Arc::new(SourceKind::JustPath(path.head.clone()))),
            _ => self
                .0
                .get(&path)
                .cloned()
                .with_context(|| format!("{path:?} not found in [{:#?}]", self.0.keys().collect_vec())),
        }
        .with_context(|| format!("when getting path [{path:?}] out of a preheated archive"))
    }
    #[tracing::instrument(skip(bottom_level_paths), fields(count=%bottom_level_paths.len()), level = "trace")]
    pub fn preheat_archive_hash_paths(bottom_level_paths: Vec<NonEmpty<PathBuf>>) -> Result<Self> {
        fn ancestors(path: NonEmpty<PathBuf>) -> impl Iterator<Item = (NonEmpty<PathBuf>, PathBuf)> {
            fn popped<T>(mut l: NonEmpty<T>) -> Option<(NonEmpty<T>, T)> {
                l.pop().map(|i| (l, i))
            }
            std::iter::successors(popped(path), |(parent, _path)| popped(parent.clone()))
        }
        let bottom_level_paths_lookup = bottom_level_paths.iter().cloned().collect::<BTreeSet<_>>();

        let all_necessary_extracts = bottom_level_paths
            .into_iter()
            .flat_map(ancestors)
            .unique()
            .sorted_by_cached_key(|(parent, _)| parent.clone())
            .collect_vec();

        let all_necessary_extracts_span = info_span!("files_to_preheat")
            .tap_mut(|pb| {
                pb.pb_set_style(&count_progress_style());
                pb.pb_set_length(all_necessary_extracts.len() as _);
            })
            .entered();
        all_necessary_extracts
            .into_iter()
            .chunk_by(|(parent, _path)| parent.clone())
            .into_iter()
            .map(|(parent, paths)| (parent, paths.into_iter().map(|(_, path)| path).collect_vec()))
            .collect::<IndexMap<_, _>>()
            .into_iter()
            .sorted_by_key(|(parent, _)| parent.len())
            .chunk_by(|(parent, _)| parent.len())
            .into_iter()
            .map(|(len, chunk)| (len, chunk.into_iter().collect_vec()))
            .try_fold(
                (BTreeMap::<_, Arc<SourceKind>>::new(), Vec::new()),
                |(mut previous_nesting_level, mut buffer), (current_len, next_chunk_by_length)| {
                    info_span!("preheating_archives_in_chunk_by_length", nesting_level=%current_len).in_scope(|| {
                        next_chunk_by_length
                            .into_iter()
                            .map(|(parent, paths)| {
                                (match parent.tail.as_slice() {
                                    &[] => parent
                                        .head
                                        .clone()
                                        .pipe(SourceKind::JustPath)
                                        .pipe(Arc::new)
                                        .pipe(Ok),
                                    _more => previous_nesting_level
                                        .get(&parent)
                                        .with_context(|| format!("parent not preheated: {parent:#?}"))
                                        .cloned(),
                                })
                                .map(|resolved_parent| (resolved_parent, parent, paths))
                            })
                            .collect::<Result<Vec<_>>>()
                            .map(|tasks| {
                                tasks
                                    .into_iter()
                                    .flat_map(|(a, b, c)| {
                                        c.tap_mut(|files| files.shuffle(&mut rand::thread_rng()))
                                            .into_iter()
                                            // TODO: this is guesstimated, ideally they would be chunked by actual size
                                            .chunks(64)
                                            .into_iter()
                                            .map(move |c| (a.clone(), b.clone(), c.collect_vec()))
                                            .collect_vec()
                                    })
                                    .collect_vec()
                            })
                            .and_then(|tasks| {
                                let performing_tasks = info_span!("performing_tasks", count=%tasks.len()).tap_mut(|pb| {
                                    pb.pb_set_style(&count_progress_style());
                                    pb.pb_set_length(tasks.len() as _);
                                });
                                performing_tasks.in_scope(|| {
                                    let performing_task = info_span!("performing_task");
                                    tasks
                                        .into_par_iter()
                                        .map({
                                            move |(archive, parent, archive_paths)| {
                                                performing_task.in_scope(|| {
                                                    info_span!("task", ?archive, ?parent, archive_paths=%archive_paths.len()).in_scope(|| {
                                                        archive_paths
                                                            .iter()
                                                            .map(|p| p.as_path())
                                                            .collect_vec()
                                                            .pipe_ref(|archive_paths| {
                                                                crate::compression::ArchiveHandle::with_guessed(
                                                                    archive.as_ref().as_ref(),
                                                                    parent.last().extension(),
                                                                    |mut archive| {
                                                                        let kind = ArchiveHandleKind::from(&archive);
                                                                        archive
                                                                            .get_many_handles(archive_paths)
                                                                            .and_then(|handles| {
                                                                                handles
                                                                                    .into_iter()
                                                                                    .map(|(path, mut file)| {
                                                                                        file.size()
                                                                                            .context("checking size")
                                                                                            .and_then(|size| file.seek_with_temp_file_blocking_raw(size))
                                                                                            .map(|e| (path, e))
                                                                                    })
                                                                                    .collect::<Result<Vec<_>>>()
                                                                                    .context("writing all files to temp files")
                                                                            })
                                                                            .with_context(|| format!("when unpacking files from archive [{kind:?}]"))
                                                                    },
                                                                )
                                                                .pipe(once)
                                                                .try_flat_map(|multiple_files| {
                                                                    multiple_files
                                                                        .into_iter()
                                                                        .map(|(archive_path, extracted)| {
                                                                            (
                                                                                parent
                                                                                    .clone()
                                                                                    .tap_mut(|parent| parent.push(archive_path.clone())),
                                                                                extracted,
                                                                            )
                                                                        })
                                                                        .map(Ok)
                                                                })
                                                                .inspect(|res| match res.as_ref() {
                                                                    Ok(chunk) => {
                                                                        tracing::trace!(?chunk, "OK");
                                                                    }
                                                                    Err(error) => {
                                                                        tracing::error!(?error, "error occurred when preheating archives")
                                                                    }
                                                                })
                                                                .collect::<Result<Vec<(NonEmpty<PathBuf>, (u64, TempPath))>>>()
                                                                .with_context(|| {
                                                                    format!(
                                                                        "extracting from archive [{archive:?}] (parent={parent:?}, \
                                                                         archive_paths={archive_paths:#?})"
                                                                    )
                                                                })
                                                            })
                                                    })
                                                })
                                            }
                                        })
                                        .inspect(|_| performing_tasks.pb_inc(1))
                                        .inspect(|r| {
                                            if let Ok(chunk) = r.as_ref() {
                                                all_necessary_extracts_span.pb_inc(chunk.len() as _)
                                            }
                                        })
                                        .collect_into_vec(&mut buffer)
                                        .pipe(|_| {
                                            previous_nesting_level.retain(|source_path, _| {
                                                // WARN: this is the important bit, sorry
                                                // that's not split up further
                                                // ALL paths with ancestors will end up here,
                                                // but this would blow the disk out of proportion.
                                                // by filtering here we drop the temp files,
                                                // and they will be cleaned up from filesystem
                                                bottom_level_paths_lookup.contains(source_path)
                                            });

                                            buffer
                                                .drain(..)
                                                .try_fold(previous_nesting_level, |acc, next| {
                                                    next.map(|next| {
                                                        acc.tap_mut(|acc| {
                                                            acc.extend(
                                                                next.into_iter()
                                                                    .map(|(k, (_, v))| (k, v.pipe(SourceKind::CachedPath).pipe(Arc::new))),
                                                            );
                                                        })
                                                    })
                                                })
                                        })
                                        .map(|acc| (acc, buffer))
                                })
                            })
                    })
                },
            )
            .map(|(preheated, _)| preheated)
            .map(Self)
    }
}
