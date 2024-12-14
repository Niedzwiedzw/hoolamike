use {
    crate::{
        downloaders::WithArchiveDescriptor,
        error::TotalResult,
        modlist_json::DirectiveKind,
        progress_bars::{vertical_progress_bar, ProgressKind, PROGRESS_BAR},
    },
    anyhow::{Context, Result},
    futures::{FutureExt, StreamExt, TryFutureExt, TryStreamExt},
    itertools::Itertools,
    nested_archive_manager::NestedArchivesService,
    std::{
        collections::{BTreeMap, HashSet},
        future::ready,
        ops::{Div, Mul},
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    },
    tap::prelude::*,
    tokio::sync::Mutex,
    tracing::{debug, info, trace},
};

pub(crate) fn create_file_all(path: &Path) -> Result<std::fs::File> {
    path.parent()
        .map(|parent| std::fs::create_dir_all(parent).with_context(|| format!("creating directory for [{}]", parent.display())))
        .unwrap_or_else(|| Ok(()))
        .and_then(|_| {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .with_context(|| format!("creating file [{}]", path.display()))
        })
}

pub mod create_bsa {
    use {super::*, crate::modlist_json::directive::CreateBSADirective};

    #[derive(Clone, Debug)]
    pub struct CreateBSAHandler {}

    impl CreateBSAHandler {
        pub fn handle(self, directive: CreateBSADirective) -> Result<()> {
            anyhow::bail!("[CreateBSADirective] {directive:#?} is not implemented")
        }
    }
}

pub type DownloadSummary = Arc<BTreeMap<String, WithArchiveDescriptor<PathBuf>>>;

pub mod from_archive;

pub mod inline_file;

pub mod patched_from_archive;

pub mod remapped_inline_file {
    use {super::*, crate::modlist_json::directive::RemappedInlineFileDirective};

    #[derive(Clone, Debug)]
    pub struct RemappedInlineFileHandler {}

    impl RemappedInlineFileHandler {
        pub fn handle(self, directive: RemappedInlineFileDirective) -> Result<()> {
            anyhow::bail!("[RemappedInlineFileDirective ] {directive:#?} is not implemented")
        }
    }
}

pub mod transformed_texture {
    use {super::*, crate::modlist_json::directive::TransformedTextureDirective};

    #[derive(Clone, Debug)]
    pub struct TransformedTextureHandler {}

    impl TransformedTextureHandler {
        pub fn handle(self, directive: TransformedTextureDirective) -> Result<()> {
            anyhow::bail!("[TransformedTextureDirective ] {directive:#?} is not implemented")
        }
    }
}

use crate::modlist_json::Directive;

pub type WabbajackFileHandle = Arc<tokio::sync::Mutex<crate::compression::zip::ZipArchive>>;

#[extension_traits::extension(pub trait WabbajackFileHandleExt)]
impl WabbajackFileHandle {
    fn from_archive(archive: crate::compression::zip::ZipArchive) -> Self {
        Arc::new(tokio::sync::Mutex::new(archive))
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
}

impl DirectiveKind {
    /// directives are not supposed to be executed in order, BSA directives expect stuff to be there up front no matter
    /// what their position in the list is
    pub fn priority(self) -> u8 {
        match self {
            DirectiveKind::InlineFile => 10,
            DirectiveKind::FromArchive => 11,
            DirectiveKind::PatchedFromArchive => 12,
            DirectiveKind::RemappedInlineFile => 13,
            DirectiveKind::TransformedTexture => 240,
            DirectiveKind::CreateBSA => 250,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirectivesHandlerConfig {
    pub failed_directives_whitelist: HashSet<String>,
    pub wabbajack_file: WabbajackFileHandle,
    pub output_directory: PathBuf,
}

pub mod nested_archive_manager;

fn concurrency() -> usize {
    #[cfg(not(debug_assertions))]
    {
        use std::ops::{Div, Mul};

        num_cpus::get().div(10).mul(8).saturating_sub(1).max(1)
    }
    #[cfg(debug_assertions)]
    {
        1
    }
}

impl DirectivesHandler {
    #[allow(clippy::new_without_default)]
    pub fn new(config: DirectivesHandlerConfig, sync_summary: Vec<WithArchiveDescriptor<PathBuf>>) -> Self {
        let DirectivesHandlerConfig {
            failed_directives_whitelist,
            wabbajack_file,
            output_directory,
        } = config.clone();
        let download_summary: DownloadSummary = sync_summary
            .into_iter()
            .map(|s| (s.descriptor.hash.clone(), s))
            .collect::<BTreeMap<_, _>>()
            .pipe(Arc::new);

        let nested_archive_service = NestedArchivesService::new(download_summary.clone(), concurrency() * 3)
            .pipe(Mutex::new)
            .pipe(Arc::new);
        Self {
            config,
            create_bsa: create_bsa::CreateBSAHandler {},
            from_archive: from_archive::FromArchiveHandler {
                output_directory: output_directory.clone(),
                nested_archive_service: nested_archive_service.clone(),
            },
            inline_file: inline_file::InlineFileHandler {
                wabbajack_file: wabbajack_file.clone(),
                output_directory: output_directory.clone(),
            },
            patched_from_archive: patched_from_archive::PatchedFromArchiveHandler {
                output_directory: output_directory.clone(),
                wabbajack_file,
                nested_archive_service,
            },
            remapped_inline_file: remapped_inline_file::RemappedInlineFileHandler {},
            transformed_texture: transformed_texture::TransformedTextureHandler {},
        }
    }
    #[tracing::instrument(skip(self))]
    pub async fn handle(self: Arc<Self>, directive: Directive) -> Result<()> {
        match directive {
            Directive::CreateBSA(directive) => self.create_bsa.clone().handle(directive),
            Directive::FromArchive(directive) => self.from_archive.clone().handle(directive).await,
            Directive::InlineFile(directive) => self.inline_file.clone().handle(directive).await,
            Directive::PatchedFromArchive(directive) => self.patched_from_archive.clone().handle(directive).await,
            Directive::RemappedInlineFile(directive) => self.remapped_inline_file.clone().handle(directive),
            Directive::TransformedTexture(directive) => self.transformed_texture.clone().handle(directive),
        }
    }
    #[allow(clippy::unnecessary_literal_unwrap)]
    pub async fn handle_directives(self: Arc<Self>, directives: Vec<Directive>) -> TotalResult<()> {
        let pb = vertical_progress_bar(
            directives.iter().map(directive_size).sum(),
            ProgressKind::InstallDirectives,
            indicatif::ProgressFinish::AndClear,
        )
        .attach_to(&PROGRESS_BAR)
        .tap_mut(|pb| {
            pb.set_message("TOTAL");
            pb.enable_steady_tick(Duration::from_secs(2));
        });

        fn directive_size(d: &Directive) -> u64 {
            match d {
                Directive::CreateBSA(directive) => directive.size,
                Directive::FromArchive(directive) => directive.size,
                Directive::InlineFile(directive) => directive.size,
                Directive::PatchedFromArchive(directive) => directive.size,
                Directive::RemappedInlineFile(directive) => directive.size,
                Directive::TransformedTexture(directive) => directive.size,
            }
        }

        // let whitelist_failed_directives = self
        //     .config
        //     .failed_directives_whitelist
        //     .clone()
        //     .pipe(Arc::new);
        directives
            .into_iter()
            .collect_vec()
            .tap_mut(|directives| {
                // directives.shuffle(&mut rand::thread_rng());
                directives.sort_unstable_by_key(|directive| DirectiveKind::from(directive).priority());
            })
            .pipe(futures::stream::iter)
            .map(move |directive| {
                let directive_hash = directive.directive_hash();
                let directive_size = directive_size(&directive);
                let directive_debug = format!("{directive:#?}").pipe(Arc::new);
                self.clone()
                    .handle(directive)
                    .map({
                        let directive_debug = directive_debug.clone();
                        let directive_hash = directive_hash.clone();
                        move |r| {
                            r.with_context(|| format!("when handling directive: {directive_debug}"))
                                .with_context(|| format!("directive with hash [{directive_hash}] failed, provide it in support ticket"))
                        }
                    })
                    .inspect_ok({
                        let directive_debug = directive_debug.clone();
                        move |_handled| trace!("handled directive {directive_debug}")
                    })
                    // .map({
                    //     let whitelist_failed_directives = whitelist_failed_directives.clone();
                    //     let directive_debug = directive_debug.clone();
                    //     move |res| match res {
                    //         Err(e) if whitelist_failed_directives.contains(&directive_hash) => {
                    //             tracing::warn!("directive\n[{directive_debug}]\nfailed with\n{e:?}\nbut is whitelisted\n\n");
                    //             Ok(())
                    //         }
                    //         other => other,
                    //     }
                    // })
                    .map_ok(move |_| directive_size)
            })
            .map(Ok)
            .try_buffered(concurrency())
            .try_for_each(|size| {
                pb.inc(size);
                ready(Ok(()))
            })
            .inspect_err(|message| tracing::error!(?message))
            .await
            .pipe(|r| match r {
                Ok(_) => Ok(vec![()]),
                Err(e) => Err(vec![e]),
            })
    }
}
