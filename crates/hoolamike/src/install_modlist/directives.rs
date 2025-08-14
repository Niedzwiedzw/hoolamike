use {
    super::download_cache::validate_hash_wabbajack,
    crate::{
        downloaders::WithArchiveDescriptor,
        install_modlist::io_progress_style,
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
        tokio_runtime_multi,
        utils::{MaybeWindowsPath, PathReadWrite},
    },
    anyhow::{Context, Result},
    futures::{FutureExt, Stream, StreamExt},
    itertools::Itertools,
    nonempty::NonEmpty,
    rayon::iter::{IntoParallelIterator, ParallelIterator},
    remapped_inline_file::RemappingContext,
    std::{
        collections::BTreeMap,
        future::ready,
        iter::once,
        path::{Path, PathBuf},
        sync::Arc,
    },
    tap::prelude::*,
    tracing::{info_span, instrument, Instrument},
    tracing_indicatif::span_ext::IndicatifSpanExt,
    transformed_texture::TexconvWineState,
    wabbajack_file_handle::WabbajackFileHandle,
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

pub mod wabbajack_file_handle;

pub struct DirectivesHandler {
    #[allow(dead_code)]
    pub config: DirectivesHandlerConfig,
    pub create_bsa: create_bsa::CreateBSAHandler,
    pub from_archive: from_archive::FromArchiveHandler,
    pub inline_file: inline_file::InlineFileHandler,
    pub patched_from_archive: patched_from_archive::PatchedFromArchiveHandler,
    pub remapped_inline_file: remapped_inline_file::RemappedInlineFileHandler,
    pub transformed_texture: transformed_texture::TransformedTextureHandler,
    pub download_summary: DownloadSummary,
}

#[derive(Debug, Clone)]
pub struct DirectivesHandlerConfig {
    pub wabbajack_file: WabbajackFileHandle,
    pub output_directory: PathBuf,
    pub game_directory: PathBuf,
    pub downloads_directory: PathBuf,
    pub texconv_wine_state: Option<TexconvWineState>,
}

pub mod nested_archive_manager;

pub fn concurrency() -> usize {
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
        false => validate_hash_wabbajack(path, hash).await,
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

pub mod nested_archive_directives;

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

pub mod preheat_archive_hash_paths;

impl DirectivesHandler {
    #[allow(clippy::new_without_default)]
    pub fn new(config: DirectivesHandlerConfig, sync_summary: Vec<WithArchiveDescriptor<PathBuf>>) -> Self {
        let DirectivesHandlerConfig {
            wabbajack_file,
            output_directory,
            game_directory,
            downloads_directory,
            texconv_wine_state,
        } = config.clone();
        let download_summary: DownloadSummary = sync_summary
            .into_iter()
            .map(|s| (s.descriptor.hash.clone(), s))
            .collect::<BTreeMap<_, _>>()
            .pipe(Arc::new);

        Self {
            config,
            create_bsa: create_bsa::CreateBSAHandler {
                output_directory: output_directory.clone(),
            },
            from_archive: from_archive::FromArchiveHandler {
                output_directory: output_directory.clone(),
                download_summary: download_summary.clone(),
            },
            inline_file: inline_file::InlineFileHandler {
                wabbajack_file: wabbajack_file.clone(),
                output_directory: output_directory.clone(),
            },
            patched_from_archive: patched_from_archive::PatchedFromArchiveHandler {
                output_directory: output_directory.clone(),
                wabbajack_file: wabbajack_file.clone(),
                download_summary: download_summary.clone(),
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
                texconv_wine_state,
            },
            download_summary,
        }
    }

    #[allow(clippy::unnecessary_literal_unwrap)]
    #[instrument(skip_all, fields(directives=%directives.len()))]
    pub fn handle_directives(self: Arc<Self>, directives: Vec<Directive>) -> Result<Vec<u64>> {
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
        {
            let validating_hashes = info_span!("validating_hashes").tap_mut(|pb| {
                pb.pb_set_style(&count_progress_style());
                pb.pb_set_length(directives.len() as _);
            });
            directives
                .pipe(futures::stream::iter)
                .map(check_completed)
                .buffer_unordered(num_cpus::get())
                .inspect({
                    cloned![validating_hashes];
                    move |_| validating_hashes.pb_inc(1)
                })
                .collect::<Vec<_>>()
                .instrument(validating_hashes)
                .pipe(|tasks| tokio_runtime_multi(concurrency()).map(|runtime| runtime.block_on(tasks)))
        }
        .map(|directives| {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()).pipe(
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
                                    Directive::PatchedFromArchive(patched_from_archive_directive) => patched_from_archive.push(patched_from_archive_directive),
                                    Directive::RemappedInlineFile(remapped_inline_file_directive) => remapped_inline_file.push(remapped_inline_file_directive),
                                    Directive::TransformedTexture(transformed_texture_directive) => transformed_texture.push(transformed_texture_directive),
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
        })
        .and_then(
            |(create_bsa, from_archive, inline_file, patched_from_archive, remapped_inline_file, transformed_texture, completed)| {
                rayon::iter::empty()
                    .chain(completed.into_par_iter().map(Ok))
                    .chain(inline_file.into_par_iter().map({
                        cloned![manager];
                        move |directive| {
                            manager
                                .clone()
                                .inline_file
                                .clone()
                                .handle(directive.clone())
                                .with_context(|| format!("handling directive [{directive:#?}]"))
                        }
                    }))
                    .chain(
                        rayon::iter::empty()
                            .chain(
                                patched_from_archive
                                    .into_par_iter()
                                    .map(ArchivePathDirective::from),
                            )
                            .chain(from_archive.into_par_iter().map(ArchivePathDirective::from))
                            .chain(
                                transformed_texture
                                    .into_par_iter()
                                    .map(ArchivePathDirective::from),
                            )
                            .collect::<Vec<_>>()
                            .pipe(|directives| {
                                const DIRECTIVE_CHUNK_SIZE: u64 = 6 * 1024 * 1024 * 1024;
                                let download_summary = self.download_summary.clone();
                                info_span!("handling nested archive directives", total_size=%directives.len(), estimated_chunk_size_bytes=%DIRECTIVE_CHUNK_SIZE)
                                    .in_scope(|| {
                                        handle_directives.in_scope(|| {
                                            crate::utils::chunk_while(directives, |d| d.iter().map(|d| d.directive_size()).sum::<u64>() > DIRECTIVE_CHUNK_SIZE)
                                                .into_par_iter()
                                                .flat_map({
                                                    cloned![manager, download_summary];
                                                    move |directives| {
                                                        info_span!("handling nested archive directives chunk", chunk_size=%directives.len()).in_scope(|| {
                                                            nested_archive_directives::handle_nested_archive_directives(
                                                                manager.clone(),
                                                                download_summary.clone(),
                                                                directives,
                                                            )
                                                            .collect_vec()
                                                            .into_par_iter()
                                                        })
                                                    }
                                                })
                                        })
                                    })
                            }),
                    )
                    .chain(remapped_inline_file.into_par_iter().map({
                        cloned![manager];
                        move |remapped_inline_file| {
                            manager
                                .remapped_inline_file
                                .clone()
                                .handle(remapped_inline_file.clone())
                                .with_context(|| format!("handling {remapped_inline_file:#?}"))
                        }
                    }))
                    .chain(create_bsa.into_par_iter().map({
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
                                .with_context(|| format!("handling directive: [{debug}]"))
                        }
                    }))
                    .inspect({
                        move |size| {
                            if let Ok(size) = size {
                                handle_directives.pb_inc(*size);
                            }
                        }
                    })
                    .collect::<Result<Vec<_>>>()
            },
        )
    }
}
