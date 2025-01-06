use {
    crate::{
        compression::{ArchiveHandleKind, ProcessArchive, SeekWithTempFileExt},
        downloaders::{helpers::FutureAnyhowExt, WithArchiveDescriptor},
        install_modlist::{download_cache::validate_hash, io_progress_style},
        modlist_json::{
            directive::{
                create_bsa_directive::{CreateBSADirective, CreateBSADirectiveKind},
                ArchiveHashPath,
                FromArchiveDirective,
                InlineFileDirective,
                PatchedFromArchiveDirective,
                RemappedInlineFileDirective,
                TransformedTextureDirective,
            },
            DirectiveKind,
        },
        progress_bars_v2::count_progress_style,
        utils::{MaybeWindowsPath, PathReadWrite},
    },
    anyhow::{Context, Result},
    futures::{FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt},
    indexmap::IndexMap,
    itertools::Itertools,
    nonempty::NonEmpty,
    queued_archive_task::{QueuedArchiveService, SourceKind},
    rand::seq::SliceRandom,
    rayon::iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator},
    remapped_inline_file::RemappingContext,
    std::{
        collections::{BTreeMap, BTreeSet},
        future::ready,
        iter::once,
        path::{Path, PathBuf},
        sync::Arc,
    },
    tap::prelude::*,
    tempfile::TempPath,
    tracing::{info_span, instrument, Instrument},
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

pub(crate) fn create_file_all(path: &Path) -> Result<std::fs::File> {
    path.parent()
        .map(|parent| std::fs::create_dir_all(parent).with_context(|| format!("creating directory for [{}]", parent.display())))
        .unwrap_or_else(|| Ok(()))
        .and_then(|_| path.open_file_write())
        .map(|(_, f)| f)
}

pub type DownloadSummary = Arc<BTreeMap<String, WithArchiveDescriptor<PathBuf>>>;

pub mod create_bsa;
pub mod from_archive;
pub mod inline_file;
pub mod patched_from_archive;
pub mod remapped_inline_file;
pub mod transformed_texture;

use crate::modlist_json::Directive;

// pub type WabbajackFileHandle = Arc<Mutex<crate::compression::compress_tools::ArchiveHandle>>;

#[derive(Debug, Clone)]
pub struct WabbajackFileHandle {
    wabbajack_file_path: Arc<PathBuf>,
}

impl WabbajackFileHandle {
    pub(crate) fn from_archive(archive: PathBuf) -> Self {
        Self {
            wabbajack_file_path: Arc::new(archive),
        }
    }
    pub(crate) fn get_archive(&self) -> Result<crate::compression::zip::ZipArchive> {
        self.wabbajack_file_path
            .open_file_read()
            .and_then(|(at_path, _file)| crate::compression::zip::ZipArchive::new(&at_path).with_context(|| format!("opening archive at path [{at_path:#?}]")))
            .context("opening wabbajack file as archive")
    }
}

pub struct DirectivesHandler {
    pub config: DirectivesHandlerConfig,
    pub create_bsa: create_bsa::CreateBSAHandler,
    pub from_archive: from_archive::FromArchiveHandler,
    pub inline_file: inline_file::InlineFileHandler,
    pub patched_from_archive: patched_from_archive::PatchedFromArchiveHandler,
    pub remapped_inline_file: remapped_inline_file::RemappedInlineFileHandler,
    pub transformed_texture: transformed_texture::TransformedTextureHandler,
    pub download_summary: DownloadSummary,
    pub archive_extraction_queue: Arc<QueuedArchiveService>,
}

#[derive(Debug, Clone)]
pub struct DirectivesHandlerConfig {
    pub wabbajack_file: WabbajackFileHandle,
    pub output_directory: PathBuf,
    pub game_directory: PathBuf,
    pub downloads_directory: PathBuf,
}

pub mod nested_archive_manager;

fn concurrency() -> usize {
    #[cfg(not(debug_assertions))]
    {
        use std::ops::Div;

        num_cpus::get().div(2).saturating_sub(1).max(1)
    }
    #[cfg(debug_assertions)]
    {
        1
    }
}

#[extension_traits::extension(pub trait StreamTryFlatMapLocalExt)]
impl<'iter, T, E, I> I
where
    E: 'iter,
    T: 'iter,
    I: Stream<Item = Result<T, E>> + 'iter,
{
    fn try_flat_map_local<U, NewStream, F>(self, try_flat_map: F) -> impl Stream<Item = Result<U, E>> + 'iter
    where
        U: 'iter,
        NewStream: Stream<Item = Result<U, E>> + 'iter,
        F: FnOnce(T) -> NewStream + 'iter + Clone,
    {
        self.flat_map(move |e| match e {
            Ok(value) => value.pipe(try_flat_map.clone()).boxed_local(),
            Err(e) => e
                .pipe(Err)
                .pipe(ready)
                .pipe(futures::stream::once)
                .boxed_local(),
        })
    }
}
#[extension_traits::extension(pub trait StreamTryFlatMapExt)]
impl<'iter, T, E, I> I
where
    E: 'static + Send + Sync,
    T: 'static + Send + Sync,
    I: Stream<Item = Result<T, E>> + 'static + Unpin,
{
    fn try_flat_map<U, NewStream, F>(self, try_flat_map: F) -> impl Stream<Item = Result<U, E>> + Unpin
    where
        U: 'static + Send + Sync,
        NewStream: Stream<Item = Result<U, E>> + 'iter,
        F: FnOnce(T) -> NewStream + 'iter + Clone,
    {
        self.flat_map(move |e| match e {
            Ok(value) => value.pipe(try_flat_map.clone()).boxed_local(),
            Err(e) => e.pipe(Err).pipe(ready).pipe(futures::stream::once).boxed(),
        })
    }
}

const EXTENSION_HASH_WHITELIST: &[&str] = &[
    //
    // hashes won't match because we cannot use welch filter
    "dds", //
          // hashes won't match because headers are also hashed in wabbajack, and textures are resized using welch filter
          // "bsa",
];

fn is_whitelisted_by_path(path: &Path) -> bool {
    matches!(
        path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref(),
        Some(ext) if EXTENSION_HASH_WHITELIST.contains(&ext)
    )
}

pub async fn validate_hash_with_overrides(path: PathBuf, hash: String, size: u64) -> Result<PathBuf> {
    match is_whitelisted_by_path(&path) {
        true => super::download_cache::validate_file_size(path, size).await,
        false => validate_hash(path, hash).await,
    }
}

#[derive(derive_more::From, Clone, Debug)]
enum ArchivePathDirective {
    FromArchive(FromArchiveDirective),
    PatchedFromArchive(PatchedFromArchiveDirective),
    TransformedTexture(TransformedTextureDirective),
}

impl ArchivePathDirective {
    #[allow(dead_code)]
    fn directive_size(&self) -> u64 {
        match self {
            ArchivePathDirective::FromArchive(d) => d.size,
            ArchivePathDirective::PatchedFromArchive(d) => d.size,
            ArchivePathDirective::TransformedTexture(d) => d.size,
        }
    }
    fn archive_path(&self) -> &ArchiveHashPath {
        match self {
            ArchivePathDirective::FromArchive(f) => &f.archive_hash_path,
            ArchivePathDirective::PatchedFromArchive(patched_from_archive_directive) => &patched_from_archive_directive.archive_hash_path,
            ArchivePathDirective::TransformedTexture(transformed_texture_directive) => &transformed_texture_directive.archive_hash_path,
        }
    }
}

pub mod queued_archive_task;

#[instrument(skip_all)]
fn handle_nested_archive_directives(
    manager: Arc<DirectivesHandler>,
    download_summary: DownloadSummary,
    queued_archive_service: Arc<QueuedArchiveService>,
    directives: Vec<ArchivePathDirective>,
    concurrency: usize,
) -> impl Stream<Item = Result<u64>> {
    let preheat_task = {
        let preheat_directives = info_span!("preheat_directives");
        directives
            .iter()
            .map(|d| d.archive_path())
            .map(|path| download_summary.resolve_archive_path(path))
            .collect::<Result<Vec<_>>>()
            .pipe(ready)
            .and_then(|paths| {
                tokio::task::spawn_blocking(move || preheat_directives.in_scope(|| PreheatedArchiveHashPaths::preheat_archive_hash_paths(paths)))
                    .map_context("thread crashed")
                    .and_then(ready)
            })
            .map_ok({
                cloned![queued_archive_service];
                move |preheated| {
                    preheated.0.iter().for_each(|(k, v)| {
                        queued_archive_service
                            .tasks
                            .preheat(k.clone(), Ok(v.clone()))
                    })
                }
            })
    };
    let handle_directives = info_span!("handle_directives");
    preheat_task.into_stream().try_flat_map(move |_| {
        directives
            .pipe(futures::stream::iter)
            .map(move |directive| match directive {
                ArchivePathDirective::TransformedTexture(transformed_texture) => manager
                    .transformed_texture
                    .clone()
                    .handle(transformed_texture.clone())
                    .instrument(handle_directives.clone())
                    .map(move |res| res.with_context(|| format!("handling directive: {transformed_texture:#?}")))
                    .boxed(),
                ArchivePathDirective::FromArchive(from_archive) => manager
                    .from_archive
                    .clone()
                    .handle(from_archive.clone())
                    .instrument(handle_directives.clone())
                    .map(move |res| res.with_context(|| format!("handling directive: {from_archive:#?}")))
                    .boxed(),
                ArchivePathDirective::PatchedFromArchive(patched_from_archive_directive) => manager
                    .patched_from_archive
                    .clone()
                    .handle(patched_from_archive_directive.clone())
                    .instrument(handle_directives.clone())
                    .map(move |res| res.with_context(|| format!("handling directive: {patched_from_archive_directive:#?}")))
                    .boxed(),
            })
            .buffer_unordered(concurrency)
    })
}

#[extension_traits::extension(pub (crate) trait ResolvePathExt)]
impl DownloadSummary {
    fn resolve_archive_path(&self, ArchiveHashPath { source_hash, path }: &ArchiveHashPath) -> Result<NonEmpty<PathBuf>> {
        self.get(source_hash)
            .with_context(|| format!("no [{source_hash}] in downloads"))
            .map(|parent| NonEmpty::new(parent.inner.clone()).tap_mut(|resolved| resolved.extend(path.iter().cloned().map(MaybeWindowsPath::into_path))))
    }
}

pub fn boxed_iter<'a, T: 'a>(iter: impl Iterator<Item = T> + 'a) -> Box<dyn Iterator<Item = T> + 'a> {
    Box::new(iter)
}

#[extension_traits::extension(pub trait IteratorTryFlatMapExt)]
impl<'iter, T, E, I> I
where
    E: 'iter,
    T: 'iter,
    I: Iterator<Item = Result<T, E>> + 'iter,
{
    fn try_flat_map<U, NewIterator, F>(self, mut try_flat_map: F) -> impl Iterator<Item = Result<U, E>> + 'iter
    where
        U: 'iter,
        NewIterator: Iterator<Item = Result<U, E>> + 'iter,
        F: FnMut(T) -> NewIterator + 'iter,
    {
        self.flat_map(move |e| match e {
            Ok(value) => value.pipe(&mut try_flat_map).pipe(boxed_iter),
            Err(e) => e.pipe(Err).pipe(once).pipe(boxed_iter),
        })
    }
}

type PreheatedArchiveHashPathsInner = BTreeMap<NonEmpty<PathBuf>, Arc<SourceKind>>;

pub struct PreheatedArchiveHashPaths(Arc<PreheatedArchiveHashPathsInner>);

impl PreheatedArchiveHashPaths {
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
                                                                info_span!("extracting_archive", archive_paths=%archive_paths.len()).in_scope(|| {
                                                                    crate::compression::ArchiveHandle::with_guessed(
                                                                        archive.as_ref().as_ref(),
                                                                        parent.last().extension(),
                                                                        |mut archive| {
                                                                            let kind = ArchiveHandleKind::from(&archive);
                                                                            let span = info_span!("getting_many_handles");
                                                                            span.in_scope(|| {
                                                                                archive
                                                                                    .get_many_handles(archive_paths)
                                                                                    .and_then(|handles| {
                                                                                        handles
                                                                                            .into_iter()
                                                                                            .map(|(path, mut file)| {
                                                                                                file.size()
                                                                                                    .context("checking size")
                                                                                                    .and_then(|size| {
                                                                                                        file.seek_with_temp_file_blocking_raw(size)
                                                                                                    })
                                                                                                    .map(|e| (path, e))
                                                                                            })
                                                                                            .collect::<Result<Vec<_>>>()
                                                                                            .context("writing all files to temp files")
                                                                                    })
                                                                                    .with_context(|| format!("when unpacking files from archive [{kind:?}]"))
                                                                            })
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
            .map(Arc::new)
            .map(Self)
    }
}

impl DirectivesHandler {
    #[allow(clippy::new_without_default)]
    pub fn new(config: DirectivesHandlerConfig, sync_summary: Vec<WithArchiveDescriptor<PathBuf>>) -> Self {
        let DirectivesHandlerConfig {
            wabbajack_file,
            output_directory,
            game_directory,
            downloads_directory,
        } = config.clone();
        let download_summary: DownloadSummary = sync_summary
            .into_iter()
            .map(|s| (s.descriptor.hash.clone(), s))
            .collect::<BTreeMap<_, _>>()
            .pipe(Arc::new);
        let archive_extraction_queue = queued_archive_task::QueuedArchiveService::new(num_cpus::get());

        Self {
            config,
            create_bsa: create_bsa::CreateBSAHandler {
                output_directory: output_directory.clone(),
            },
            from_archive: from_archive::FromArchiveHandler {
                output_directory: output_directory.clone(),
                download_summary: download_summary.clone(),
                archive_extraction_queue: archive_extraction_queue.clone(),
            },
            inline_file: inline_file::InlineFileHandler {
                wabbajack_file: wabbajack_file.clone(),
                output_directory: output_directory.clone(),
            },
            patched_from_archive: patched_from_archive::PatchedFromArchiveHandler {
                output_directory: output_directory.clone(),
                wabbajack_file: wabbajack_file.clone(),
                download_summary: download_summary.clone(),
                archive_extraction_queue: archive_extraction_queue.clone(),
            },
            remapped_inline_file: remapped_inline_file::RemappedInlineFileHandler {
                remapping_context: Arc::new(RemappingContext {
                    game_folder: game_directory.clone(),
                    output_directory: output_directory.clone(),
                    downloads_directory,
                }),
                wabbajack_file: wabbajack_file.clone(),
            },
            transformed_texture: transformed_texture::TransformedTextureHandler {
                output_directory: output_directory.clone(),
                download_summary: download_summary.clone(),
                archive_extraction_queue: archive_extraction_queue.clone(),
            },
            download_summary,
            archive_extraction_queue,
        }
    }

    #[allow(clippy::unnecessary_literal_unwrap)]
    #[instrument(skip_all, fields(directives=%directives.len()))]
    pub fn handle_directives(self: Arc<Self>, directives: Vec<Directive>) -> impl Stream<Item = Result<u64>> {
        let handle_directives: &'static _ = tracing::Span::current()
            .tap(|pb| {
                pb.pb_set_length(directives.iter().map(directive_size).sum());
                pb.pb_set_style(&io_progress_style());
            })
            .pipe(Box::new)
            .pipe(Box::leak);

        fn directive_size(d: &Directive) -> u64 {
            match d {
                Directive::CreateBSA(directive) => directive.size(),
                Directive::FromArchive(directive) => directive.size,
                Directive::InlineFile(directive) => directive.size,
                Directive::PatchedFromArchive(directive) => directive.size,
                Directive::RemappedInlineFile(directive) => directive.size,
                Directive::TransformedTexture(directive) => directive.size,
            }
        }
        let manager = self.clone();

        enum DirectiveStatus {
            Completed(u64),
            NeedsRebuild { reason: anyhow::Error, directive: Directive },
        }

        let check_completed = {
            let output_directory = self.from_archive.output_directory.clone();
            move |directive: Directive| {
                let _kind = DirectiveKind::from(&directive);
                match &directive {
                    Directive::CreateBSA(create_bsa) => match create_bsa {
                        CreateBSADirective::Bsa(CreateBSADirectiveKind { hash, size, to, .. }) => (hash.clone(), *size, to.clone()),
                        CreateBSADirective::Ba2(CreateBSADirectiveKind { hash, size, to, .. }) => (hash.clone(), *size, to.clone()),
                    },
                    Directive::FromArchive(FromArchiveDirective { hash, size, to, .. }) => (hash.clone(), *size, to.clone()),
                    Directive::InlineFile(InlineFileDirective { hash, size, to, .. }) => (hash.clone(), *size, to.clone()),
                    Directive::PatchedFromArchive(PatchedFromArchiveDirective { hash, size, to, .. }) => (hash.clone(), *size, to.clone()),
                    Directive::RemappedInlineFile(RemappedInlineFileDirective { hash, size, to, .. }) => (hash.clone(), *size, to.clone()),
                    Directive::TransformedTexture(TransformedTextureDirective { hash, size, to, .. }) => (hash.clone(), *size, to.clone()),
                }
                .pipe(|(hash, size, to)| (hash, size, output_directory.join(to.into_path())))
                .pipe(move |(hash, size, to)| {
                    validate_hash_with_overrides(to.clone(), hash, size)
                        .map(move |res| match res {
                            Ok(_) => DirectiveStatus::Completed(size),
                            Err(reason) => DirectiveStatus::NeedsRebuild { reason, directive },
                        })
                        .instrument(handle_directives.clone())
                })
            }
        };
        directives
            .pipe(futures::stream::iter)
            .map(check_completed)
            .buffer_unordered(num_cpus::get())
            .collect::<Vec<_>>()
            .then(|directives| {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
                    .pipe(
                        |(
                            mut create_bsa,
                            mut from_archive,
                            mut inline_file,
                            mut patched_from_archive,
                            mut remapped_inline_file,
                            mut transformed_texture,
                            mut completed,
                        )| {
                            directives
                                .into_iter()
                                .for_each(|directive| match directive {
                                    DirectiveStatus::Completed(size) => completed.push(size),
                                    DirectiveStatus::NeedsRebuild { reason, directive } => {
                                        tracing::debug!(
                                            "recomputing directive\ndirective:{directive}:\nreason:{reason:?}",
                                            directive = format!("{directive:#?}")
                                                .chars()
                                                .take(256)
                                                .collect::<String>(),
                                        );
                                        match directive {
                                            Directive::CreateBSA(create_bsadirective) => create_bsa.push(create_bsadirective),
                                            Directive::FromArchive(from_archive_directive) => from_archive.push(from_archive_directive),
                                            Directive::InlineFile(inline_file_directive) => inline_file.push(inline_file_directive),
                                            Directive::PatchedFromArchive(patched_from_archive_directive) => {
                                                patched_from_archive.push(patched_from_archive_directive)
                                            }
                                            Directive::RemappedInlineFile(remapped_inline_file_directive) => {
                                                remapped_inline_file.push(remapped_inline_file_directive)
                                            }
                                            Directive::TransformedTexture(transformed_texture_directive) => {
                                                transformed_texture.push(transformed_texture_directive)
                                            }
                                        }
                                    }
                                })
                                .pipe(|_| {
                                    (
                                        create_bsa,
                                        from_archive,
                                        inline_file,
                                        patched_from_archive,
                                        remapped_inline_file,
                                        transformed_texture,
                                        completed,
                                    )
                                })
                        },
                    )
                    .pipe(ready)
            })
            .into_stream()
            .flat_map(
                move |(create_bsa, from_archive, inline_file, patched_from_archive, remapped_inline_file, transformed_texture, completed)| {
                    futures::stream::empty()
                        .chain(completed.pipe(futures::stream::iter).map(Ok))
                        .chain(
                            inline_file
                                .pipe(futures::stream::iter)
                                .map({
                                    cloned![manager];
                                    move |directive| {
                                        manager
                                            .clone()
                                            .inline_file
                                            .clone()
                                            .handle(directive.clone())
                                            .instrument(handle_directives.clone())
                                            .map(move |res| res.with_context(|| format!("handling directive [{directive:#?}]")))
                                    }
                                })
                                .buffer_unordered(concurrency()),
                        )
                        .chain(
                            std::iter::empty()
                                .chain(
                                    patched_from_archive
                                        .into_iter()
                                        .map(ArchivePathDirective::from),
                                )
                                .chain(from_archive.into_iter().map(ArchivePathDirective::from))
                                .chain(
                                    transformed_texture
                                        .into_iter()
                                        .map(ArchivePathDirective::from),
                                )
                                .collect_vec()
                                .pipe(|directives| {
                                    handle_directives.in_scope(|| {
                                        handle_nested_archive_directives(
                                            manager.clone(),
                                            self.download_summary.clone(),
                                            self.archive_extraction_queue.clone(),
                                            directives,
                                            concurrency(),
                                        )
                                    })
                                }),
                        )
                        .chain(
                            remapped_inline_file
                                .pipe(futures::stream::iter)
                                .map({
                                    cloned![manager];
                                    move |remapped_inline_file| {
                                        manager
                                            .remapped_inline_file
                                            .clone()
                                            .handle(remapped_inline_file.clone())
                                            .instrument(handle_directives.clone())
                                            .map(move |res| res.with_context(|| format!("handling {remapped_inline_file:#?}")))
                                    }
                                })
                                .buffer_unordered(concurrency()),
                        )
                        .chain(create_bsa.pipe(futures::stream::iter).then({
                            cloned![manager];
                            move |create_bsa| {
                                let debug = format!("{create_bsa:#?}")
                                    .chars()
                                    .take(256)
                                    .collect::<String>();
                                manager
                                    .create_bsa
                                    .clone()
                                    .handle(create_bsa)
                                    .instrument(handle_directives.clone())
                                    .map(move |res| res.with_context(|| format!("handling directive: [{debug}]")))
                            }
                        }))
                        .inspect_ok({
                            move |size| {
                                handle_directives.pb_inc(*size);
                            }
                        })
                },
            )
    }
}
