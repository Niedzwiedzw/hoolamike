use {
    crate::{
        downloaders::WithArchiveDescriptor,
        error::{MultiErrorCollectExt, TotalResult},
    },
    anyhow::{Context, Result},
    futures::{FutureExt, StreamExt, TryFutureExt, TryStreamExt},
    std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
        sync::Arc,
    },
    tap::prelude::*,
    tracing::{debug, info},
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
    pub create_bsa: create_bsa::CreateBSAHandler,
    pub from_archive: from_archive::FromArchiveHandler,
    pub inline_file: inline_file::InlineFileHandler,
    pub patched_from_archive: patched_from_archive::PatchedFromArchiveHandler,
    pub remapped_inline_file: remapped_inline_file::RemappedInlineFileHandler,
    pub transformed_texture: transformed_texture::TransformedTextureHandler,
}

impl DirectivesHandler {
    #[allow(clippy::new_without_default)]
    pub fn new(wabbajack_file: WabbajackFileHandle, output_directory: PathBuf, sync_summary: Vec<WithArchiveDescriptor<PathBuf>>) -> Self {
        let download_summary = sync_summary
            .into_iter()
            .map(|s| (s.descriptor.hash.clone(), s))
            .collect::<BTreeMap<_, _>>()
            .pipe(Arc::new);
        Self {
            create_bsa: create_bsa::CreateBSAHandler {},
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
                wabbajack_file,
                download_summary: download_summary.clone(),
            },
            remapped_inline_file: remapped_inline_file::RemappedInlineFileHandler {},
            transformed_texture: transformed_texture::TransformedTextureHandler {},
        }
    }
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
        directives
            .pipe(futures::stream::iter)
            .then(|directive| {
                let directive_debug = format!("{directive:#?}");
                debug!("handling directive {directive_debug}");
                self.clone()
                    .handle(directive)
                    .map({
                        let directive_debug = directive_debug.clone();
                        move |r| r.with_context(|| format!("when handling directive: {directive_debug}"))
                    })
                    .inspect_ok(move |_handled| info!("handled directive {directive_debug}"))
            })
            .map_err(|e| Err(e).expect("all directives must be handled"))
            .multi_error_collect()
            .await
    }
}
