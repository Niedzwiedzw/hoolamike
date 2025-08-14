use {
    crate::{
        config_file::{HoolamikeConfig, InstallationConfig},
        consts::TEMP_FILE_DIR,
        downloaders::WithArchiveDescriptor,
        error::TotalResult,
        extensions::texconv_wine,
        modlist_json::{Archive, HumanUrl, Modlist},
        progress_bars_v2::io_progress_style,
        tokio_runtime_multi,
        wabbajack_file::WabbajackFile,
        DebugHelpers,
    },
    anyhow::Context,
    directives::{concurrency, transformed_texture::TexconvWineState, DirectivesHandler, DirectivesHandlerConfig},
    download_cache::validate_hash_sha512,
    downloads::{stream_file_validate, Synchronizers},
    futures::{FutureExt, TryFutureExt},
    itertools::Itertools,
    std::{future::ready, path::Path, sync::Arc},
    tap::prelude::*,
    tokio_stream::StreamExt,
    tracing::{info, info_span, instrument},
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

pub mod directives;
pub mod download_cache;
pub mod downloads;

#[instrument]
fn setup_texconv_wine(at: &Path, texconv_wine::ExtensionConfig { wine_path, texconv_path }: texconv_wine::ExtensionConfig) -> anyhow::Result<TexconvWineState> {
    #[rustfmt::skip]
    const TEXCONV_DEPS: &[(&str, &str, Option<&str>, &[&str])] = &[
        (
            "https://aka.ms/vs/17/release/vc_redist.x64.exe",
            "vc_redist.x64.exe",
            None,
            &["/q"],
        ),
        (
            "https://builds.dotnet.microsoft.com/dotnet/WindowsDesktop/9.0.7/windowsdesktop-runtime-9.0.7-win-x64.exe",
            "windowsdesktop-runtime-9.0.7-win-x64.exe",
            None,
            &["/quiet", "/passive", "/norestart"],
        ),
    ];
    TEXCONV_DEPS
        .pipe(futures::stream::iter)
        .then(async |(url, name, expected_hash, args)| {
            info!("downloading {url}");
            let _span = info_span!("downloading installer", %url, %name).entered();
            url.parse::<HumanUrl>()
                .with_context(|| format!("parsing url [{url}]"))
                .pipe(ready)
                .and_then(|url| {
                    stream_file_validate(url, at.join(name), None).and_then(async |file| match expected_hash {
                        Some(expected_hash) => validate_hash_sha512(file.clone(), expected_hash).await,
                        None => Ok(file),
                    })
                })
                .await
                .with_context(|| format!("downloading [{url}]"))
                .map(|path| (path, *args))
        })
        .collect::<anyhow::Result<Vec<_>>>()
        .pipe(|task| tokio_runtime_multi(TEXCONV_DEPS.len().max(1)).and_then(|rt| rt.block_on(task)))
        .and_then(|downloaded| {
            let canonicalize = |path: &Path| std::fs::canonicalize(path).with_context(|| format!("could not canonicalize [{path:?}]"));
            anyhow::Ok(TexconvWineState {
                texconv_path: texconv_path.pipe_deref(canonicalize)?,
                wine_prefix_state: wine_wrapper::wine_context::WineContext {
                    wine_path,
                    show_gui: false,
                    prefix_dir: tempfile::Builder::new()
                        .prefix("pfx-")
                        .tempdir_in(*TEMP_FILE_DIR)
                        .context("creating temp directory for prefix")
                        .map(Arc::new)?,
                }
                .initialize_with_installs(&downloaded)
                .context("could not initialize wine context for texconv")
                .map(Arc::new)?,
            })
        })
}

#[allow(clippy::needless_as_bytes)]
#[instrument(skip_all)]
pub fn install_modlist(
    HoolamikeConfig {
        downloaders,
        installation: InstallationConfig {
            wabbajack_file_path,
            installation_path,
        },
        games,
        fixup: _,
        extras,
    }: HoolamikeConfig,
    DebugHelpers {
        skip_verify_and_downloads,
        start_from_directive,
        skip_kind,
        contains,
    }: DebugHelpers,
) -> TotalResult<()> {
    std::fs::create_dir_all(&installation_path)
        .with_context(|| format!("creating installation_path: {installation_path:?}"))
        .map_err(|e| vec![e])?;

    let texconv_wine_state = extras
        .as_ref()
        .and_then(|extras| extras.texconv_wine.as_ref())
        .cloned()
        .map(|texconv_config| setup_texconv_wine(&installation_path, texconv_config))
        .transpose()
        .context("texconv config was specified, but it could not be set up")
        .map_err(|e| vec![e])?;

    let synchronizers = Synchronizers::new(downloaders.clone(), games.clone())
        .context("setting up downloaders")
        .map_err(|e| vec![e])?;
    let (
        wabbajack_file_handle,
        WabbajackFile {
            wabbajack_file_path: _,
            wabbajack_entries: _,
            modlist,
        },
    ) = WabbajackFile::load_wabbajack_file(wabbajack_file_path)
        .context("loading modlist file")
        .tap_ok(|(_, wabbajack)| {
            // PROGRESS
            wabbajack
                .modlist
                .archives
                .iter()
                .map(|archive| archive.descriptor.size)
                .chain(
                    wabbajack
                        .modlist
                        .directives
                        .iter()
                        .map(|directive| directive.size()),
                )
                .sum::<u64>()
                .pipe(|total_size| {
                    tracing::Span::current().pipe_ref(|pb| {
                        pb.pb_set_style(&io_progress_style());
                        pb.pb_set_length(total_size);
                    });
                })
        })
        .map_err(|e| vec![e])?;

    modlist.pipe(Ok).and_then(
        move |Modlist {
                  archives,
                  author: _,
                  description: _,
                  directives,
                  game_type,
                  image: _,
                  is_nsfw: _,
                  name: _,
                  readme: _,
                  version: _,
                  wabbajack_version: _,
                  website: _,
              }| {
            // let archives: Vec<_> = archives
            //     .into_iter()
            //     .filter(|archive| {
            //         serde_json::to_string(&archive)
            //             .tap_err(|e| tracing::error!("{e:#?}"))
            //             .map(|directive| contains.iter().all(|contains| directive.contains(contains)))
            //             .unwrap_or(false)
            //     })
            //     .collect();
            match skip_verify_and_downloads {
                true => archives
                    .into_iter()
                    .map(|Archive { descriptor, state: _ }| WithArchiveDescriptor {
                        inner: synchronizers
                            .cache
                            .download_output_path(descriptor.name.clone()),
                        descriptor,
                    })
                    .collect_vec()
                    .pipe(Ok)
                    .pipe(ready)
                    .boxed_local(),
                false => synchronizers.clone().sync_downloads(archives).boxed_local(),
            }
            .pipe(|tasks| {
                tokio_runtime_multi(concurrency())
                    .map_err(|e| vec![e])
                    .and_then(|r| r.block_on(tasks))
            })
            .and_then({
                move |summary| {
                    tracing::Span::current().pb_inc(summary.iter().map(|d| d.descriptor.size).sum());
                    games
                        .get(&game_type)
                        .with_context(|| format!("[{game_type}] not found in {:?}", games.keys().collect::<Vec<_>>()))
                        .map(|game_config| {
                            DirectivesHandler::new(
                                DirectivesHandlerConfig {
                                    wabbajack_file: wabbajack_file_handle,
                                    output_directory: installation_path,
                                    game_directory: game_config.root_directory.clone(),
                                    downloads_directory: downloaders.downloads_directory.clone(),
                                    texconv_wine_state,
                                },
                                summary,
                            )
                        })
                        .map_err(|e| vec![e])
                }
            })
            .map(Arc::new)
            .and_then(move |directives_handler| {
                directives_handler
                    .handle_directives(directives.tap_mut(|directives| {
                        *directives = directives
                            .pipe(std::mem::take)
                            .drain(..)
                            .skip_while(|d| {
                                start_from_directive
                                    .as_ref()
                                    .map(|start_from_directive| &d.directive_hash() != start_from_directive)
                                    .unwrap_or(false)
                            })
                            .filter(|directive| !skip_kind.contains(&directive.directive_kind()))
                            .filter(|directive| {
                                serde_json::to_string(&directive)
                                    .tap_err(|e| tracing::error!("{e:#?}"))
                                    .map(|directive| contains.iter().all(|contains| directive.contains(contains)))
                                    .unwrap_or(false)
                            })
                            .collect_vec();
                    }))
                    .map(|sizes| {
                        sizes
                            .into_iter()
                            .for_each(|size| tracing::Span::current().pb_inc(size))
                    })
                    .map(|_| vec![()])
                    .map_err(|err| vec![err])
            })
        },
    )
}
