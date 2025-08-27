use {
    crate::{
        downloaders::wabbajack_cdn::WabbajackCDNDownloader,
        install_modlist::downloads::stream_file_validate,
        modlist_json::{HumanUrl, WabbajackCDNDownloaderState},
        utils::PathFileNameOrEmpty,
    },
    anyhow::{Context, Result},
    clap::Args,
    futures::{FutureExt, StreamExt, TryFutureExt, TryStreamExt},
    std::{future::ready, num::NonZeroUsize, path::PathBuf, sync::Arc},
    tap::{Pipe, TapFallible},
    tracing::info,
};

#[derive(Args, Clone, Debug)]
pub struct CommandArgs {
    pub url: HumanUrl,
    pub to: std::path::PathBuf,
    #[arg(default_value_t = NonZeroUsize::new(16).unwrap())]
    pub download_concurrency: NonZeroUsize,
}

impl CommandArgs {
    pub async fn download(self) -> Result<PathBuf> {
        let Self { url, to, download_concurrency } = self;
        let _ = std::fs::File::options()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&to)
            .context("checking if output file can be created")?;
        let output_file_name = to.file_name().context("output must have a file name")?;
        let temp_directory = tempfile::Builder::new()
            .prefix(&output_file_name)
            .tempdir_in(".")
            .map(Arc::new)
            .context("creating temp directory")?;

        WabbajackCDNDownloader::prepare_download(WabbajackCDNDownloaderState { url: url.clone() })
            .map(|r| r.context("fetching the source urls"))
            .and_then(|urls| {
                let chunk_count = urls.len();
                urls.pipe(futures::stream::iter)
                    .enumerate()
                    .map({
                        cloned![to, temp_directory];
                        move |(idx, url)| {
                            cloned![to, temp_directory];
                            async move {
                                to.map_file_stem(|s| format!("{s}--{idx}"))
                                    .context("bad output filename")
                                    .map(|full_path| {
                                        full_path
                                            .file_name()
                                            .expect("checked above")
                                            .pipe(|name| temp_directory.path().join(name))
                                    })
                                    .pipe(ready)
                                    .map_ok(|output_path| (url, output_path, idx))
                                    .and_then(|(url, output_path, idx)| {
                                        stream_file_validate(url, output_path, None)
                                            .map(move |r| r.with_context(|| format!("downloading part {idx}")))
                                            .map_ok(move |output| {
                                                info!("downloaded chunk {idx}/{chunk_count}");
                                                (idx, output)
                                            })
                                    })
                                    .await
                            }
                        }
                    })
                    .buffer_unordered(download_concurrency.get())
                    .try_collect::<Vec<_>>()
                    .map(|r| r.context("some downloads failed"))
                    .map_ok(|mut files| {
                        files.sort_by_cached_key(|(idx, _)| *idx);
                        files
                    })
                    .and_then({
                        cloned![to];
                        async move |files| {
                            tokio::fs::File::options()
                                .create(true)
                                .truncate(true)
                                .write(true)
                                .open(&to)
                                .map(|r| r.with_context(|| format!("could not open [{}] for writing", to.display())))
                                .and_then(async |mut output_file| {
                                    for (idx, source) in files {
                                        tokio::fs::File::open(&source)
                                            .map(|r| r.with_context(|| format!("opening chunk file at {}", source.display())))
                                            .and_then(async |mut source| {
                                                tokio::io::copy(&mut source, &mut output_file)
                                                    .map(|r| r.with_context(|| format!("merging chunk [{idx}]")))
                                                    .await
                                            })
                                            .await
                                            .tap_ok(|size| info!("wrote [{size} bytes] (chunk #{idx})"))?;
                                    }
                                    Ok(())
                                })
                                .map_ok(|_| to.clone())
                                .await
                        }
                    })
            })
            .await
            .with_context(|| format!("downloading [{url}] from wabbajack CDN in chunks into [{}]", to.display()))
    }
}
