use {
    super::{
        preheat_archive_hash_paths::PreheatedArchiveHashPaths,
        ArchivePathDirective,
        DirectivesHandler,
        DownloadSummary,
        IteratorTryFlatMapExt,
        ResolvePathExt,
    },
    anyhow::{Context, Result},
    std::{iter::once, sync::Arc},
    tap::prelude::*,
    tracing::{info_span, instrument},
};

#[instrument(skip_all)]
pub(crate) fn handle_nested_archive_directives(
    manager: Arc<DirectivesHandler>,
    download_summary: DownloadSummary,
    directives: Vec<ArchivePathDirective>,
) -> impl Iterator<Item = Result<u64>> {
    let preheat_task = {
        let preheat_directives = info_span!("preheat_directives");
        directives
            .iter()
            .map(|d| d.archive_path())
            .map(|path| download_summary.resolve_archive_path(path))
            .collect::<Result<Vec<_>>>()
            .and_then(|paths| preheat_directives.in_scope(|| PreheatedArchiveHashPaths::preheat_archive_hash_paths(paths)))
    };
    let _handle_directives = info_span!("handle_directives").entered();

    let directives: Arc<[_]> = Arc::from(directives);

    preheat_task
        .map(Arc::new)
        .pipe(once)
        .try_flat_map(move |preheated| {
            let directives = directives.as_ref().to_vec();
            directives.into_iter().map({
                cloned![manager];
                move |directive| match directive {
                    ArchivePathDirective::TransformedTexture(transformed_texture) => manager
                        .clone()
                        .transformed_texture
                        .clone()
                        .handle(transformed_texture.clone(), preheated.clone())
                        .with_context(|| format!("handling directive: {transformed_texture:#?}")),
                    ArchivePathDirective::FromArchive(from_archive) => manager
                        .clone()
                        .from_archive
                        .clone()
                        .handle(from_archive.clone(), preheated.clone())
                        .with_context(|| format!("handling directive: {from_archive:#?}")),
                    ArchivePathDirective::PatchedFromArchive(patched_from_archive_directive) => manager
                        .clone()
                        .patched_from_archive
                        .clone()
                        .handle(patched_from_archive_directive.clone(), preheated.clone())
                        .with_context(|| format!("handling directive: {patched_from_archive_directive:#?}")),
                }
            })
        })
}
