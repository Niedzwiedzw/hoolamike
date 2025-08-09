use {
    super::*,
    crate::{
        config_file::{DownloadersConfig, GamesConfig},
        downloaders::{
            gamefile_source_downloader::{get_game_file_source_synchronizers, GameFileSourceSynchronizers},
            helpers::FutureAnyhowExt,
            mediafire::MediaFireDownloader,
            nexus::{self, NexusDownloader},
            wabbajack_cdn::WabbajackCDNDownloader,
            CopyFileTask,
            DownloadTask,
            MergeDownloadTask,
            SyncTask,
            WithArchiveDescriptor,
        },
        error::{MultiErrorCollectExt, TotalResult},
        modlist_json::{Archive, GoogleDriveState, HttpState, HumanUrl, ManualState, MediaFireState, MegaState, State},
        progress_bars_v2::IndicatifWrapIoExt,
    },
    anyhow::Result,
    futures::{FutureExt, StreamExt, TryStreamExt},
    std::{collections::HashMap, path::PathBuf, sync::Arc},
    tracing::{debug, instrument, Instrument},
};

#[derive(Clone)]
pub struct DownloadersInner {
    pub nexus: Option<Arc<NexusDownloader>>,
}

impl DownloadersInner {
    pub fn new(DownloadersConfig { nexus, downloads_directory: _ }: DownloadersConfig) -> Result<Self> {
        Ok(Self {
            nexus: nexus
                .api_key
                .map(NexusDownloader::new)
                .transpose()?
                .map(Arc::new),
        })
    }
}

#[derive(Clone)]
pub struct Synchronizers {
    #[allow(dead_code)]
    pub config: Arc<DownloadersConfig>,
    inner: DownloadersInner,
    pub(crate) cache: Arc<download_cache::DownloadCache>,
    game_synchronizers: Arc<GameFileSourceSynchronizers>,
}

enum Either<L, R> {
    Left(L),
    Right(R),
}

#[instrument]
async fn copy_local_file(from: PathBuf, to: PathBuf, expected_size: u64) -> Result<PathBuf> {
    let mut source_file = tokio::fs::OpenOptions::new()
        .read(true)
        .open(&from)
        .map_with_context(|| format!("opening [{}]", from.display()))
        .or_else(|error| {
            tracing::warn_span!("could not find [{from:?}], trying case insensitive?", reason = format!("{error:?}"))
                .in_scope(|| {
                    tokio::task::block_in_place(|| {
                        from.file_name()
                            .context("no file name")
                            .and_then(|name| {
                                name.to_str()
                                    .context("filename is not a utf8 string")
                                    .map(|file_name| file_name.to_lowercase())
                            })
                            .and_then(|file_name| {
                                from.parent()
                                    .with_context(|| format!("[{from:?}] has no parent"))
                                    .and_then(|parent| {
                                        parent
                                            .read_dir()
                                            .context("reading directory")
                                            .and_then(|dir| {
                                                dir.into_iter()
                                                    .map(|dir| {
                                                        dir.context("reading entry")
                                                            .and_then(|entry| {
                                                                entry
                                                                    .file_name()
                                                                    .into_string()
                                                                    .map_err(|name| {
                                                                        anyhow::anyhow!("entry [{entry:?}] ([{name:?}]) is not a valid utf8 string")
                                                                    })
                                                                    .map(|name| (name, entry))
                                                            })
                                                            .map(|(filename, entry)| (filename.to_lowercase(), entry.path().to_owned()))
                                                    })
                                                    .collect::<Result<HashMap<_, _>>>()
                                                    .with_context(|| format!("when listing [{parent:?}]"))
                                            })
                                            .and_then(|mut paths| {
                                                paths.remove(&file_name).with_context(|| {
                                                    format!(
                                                        "no [{file_name}] in [{parent:?}] (looking up by lowercase filename, and choices are:\n[{paths:#?}])"
                                                    )
                                                })
                                            })
                                    })
                            })
                    })
                })
                .pipe(ready)
                .and_then(|lowercase| async move {
                    tokio::fs::OpenOptions::new()
                        .read(true)
                        .open(lowercase.clone())
                        .map_with_context({
                            cloned![lowercase];
                            move || format!("opening lowercase file that matched the path: [{lowercase:?}]")
                        })
                        .await
                        .tap_ok(|_| {
                            tracing::warn!(
                                "opened file matched by lowercase filename: [{lowercase:?}]. this is guesswork at this point, if this approach doesn't work \
                                 file an issue"
                            )
                        })
                })
        })
        .await?;
    let target_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&to)
        .map_with_context(|| format!("opening [{}]", to.display()))
        .await?;

    let copied = tokio::io::copy(&mut source_file, &mut tracing::Span::current().wrap_async_write(expected_size, target_file))
        .await
        .context("copying")?;

    if copied != expected_size {
        anyhow::bail!("[{from:?} -> {to:?}] local copy finished, but received unexpected size (expected [{expected_size}] bytes, downloaded [{copied} bytes])")
    }
    Ok(to)
}
#[instrument]
pub async fn stream_merge_file(from: Vec<HumanUrl>, to: PathBuf, expected_size: u64) -> Result<PathBuf> {
    let target_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&to)
        .map_with_context(|| format!("opening [{}]", to.display()))
        .await?;

    let mut writer = &mut tracing::Span::current().wrap_async_write(expected_size, target_file);
    let mut downloaded = 0;
    for from_chunk in from.clone().into_iter() {
        let mut byte_stream = reqwest::get(from_chunk.to_string())
            .await
            .with_context(|| format!("making request to {from_chunk}"))?
            .bytes_stream();
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(chunk) => {
                    downloaded += chunk.len() as u64;
                    tokio::io::copy(&mut chunk.as_ref(), &mut writer)
                        .await
                        .with_context(|| format!("writing to fd {}", to.display()))?;
                }
                Err(message) => Err(message)?,
            }
        }
    }

    if downloaded != expected_size {
        anyhow::bail!("[{from:?}] download finished, but received unexpected size (expected [{expected_size}] bytes, downloaded [{downloaded} bytes])")
    }
    Ok(to)
}

#[instrument]
pub async fn stream_file(from: HumanUrl, to: PathBuf, expected_size: u64) -> Result<PathBuf> {
    let target_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&to)
        .map_with_context(|| format!("opening [{}]", to.display()))
        .await?;
    let mut writer = &mut tracing::Span::current().wrap_async_write(expected_size, tokio::io::BufWriter::new(target_file));
    let mut byte_stream = reqwest::get(from.to_string())
        .await
        .with_context(|| format!("making request to {from}"))?
        .bytes_stream();
    let mut downloaded = 0;
    while let Some(chunk) = byte_stream.next().await {
        match chunk {
            Ok(chunk) => {
                downloaded += chunk.len() as u64;

                tokio::io::copy(&mut chunk.as_ref(), &mut writer)
                    .await
                    .with_context(|| format!("writing to fd {}", to.display()))?;
            }
            Err(message) => Err(message)?,
        }
    }
    if downloaded != expected_size {
        anyhow::bail!("[{from}] download finished, but received unexpected size (expected [{expected_size}] bytes, downloaded [{downloaded} bytes])")
    }
    Ok(to)
}
impl Synchronizers {
    pub fn new(config: DownloadersConfig, games_config: GamesConfig) -> Result<Self> {
        Ok(Self {
            config: Arc::new(config.clone()),
            cache: Arc::new(download_cache::DownloadCache::new(config.downloads_directory.clone()).context("building download cache")?),
            inner: DownloadersInner::new(config).context("building downloaders")?,
            game_synchronizers: Arc::new(get_game_file_source_synchronizers(games_config).context("building game file source synchronizers")?),
        })
    }

    pub async fn prepare_sync_task(self, Archive { descriptor, state }: Archive) -> Result<SyncTask> {
        match state.clone() {
            State::Nexus(nexus_state) => self
                .inner
                .nexus
                .clone()
                .context("nexus not configured")
                .pipe(ready)
                .and_then(|nexus| nexus.download(nexus::DownloadFileRequest::from_nexus_state(nexus_state)))
                .await
                .map(|url| DownloadTask {
                    inner: (url, self.cache.download_output_path(descriptor.name.clone())),
                    descriptor,
                })
                .map(SyncTask::from),
            State::GoogleDrive(GoogleDriveState { id }) => crate::downloaders::google_drive::GoogleDriveDownloader::download(id, descriptor.size)
                .await
                .map(|url| DownloadTask {
                    inner: (url, self.cache.download_output_path(descriptor.name.clone())),
                    descriptor,
                })
                .map(SyncTask::from),
            State::GameFileSource(state) => self
                .game_synchronizers
                .get(&state.game)
                .with_context(|| format!("check config, no game source configured for [{}]", state.game))
                .pipe(ready)
                .and_then(|synchronizer| synchronizer.prepare_copy(state))
                .await
                .map(|source_path| CopyFileTask {
                    inner: (source_path, self.cache.download_output_path(descriptor.name.clone())),
                    descriptor,
                })
                .map(SyncTask::from),

            State::Http(HttpState { url, headers: _ }) => url
                .pipe(|url| DownloadTask {
                    inner: (url, self.cache.download_output_path(descriptor.name.clone())),
                    descriptor,
                })
                .pipe(SyncTask::from)
                .pipe(Ok),
            State::WabbajackCDN(state) => WabbajackCDNDownloader::prepare_download(state)
                .await
                .context("fetching from wabbajack cdn")
                .map(|source_urls| MergeDownloadTask {
                    inner: (source_urls, self.cache.download_output_path(descriptor.name.clone())),
                    descriptor,
                })
                .map(SyncTask::from),
            State::Manual(ManualState { prompt, url }) => Err(anyhow::anyhow!("Manual action is required:\n\nURL: {url}\n{prompt}")),
            State::Mega(MegaState { url }) => Err(anyhow::anyhow!(
                "Manual action is required:\n\nURL: {url}\nMega is not supported (yet?), please download the file manually"
            )),
            State::MediaFire(MediaFireState { url }) => {
                // it cannot be done
                MediaFireDownloader::download(url.clone())
                    .await
                    .context("mediafire")
                    .map(|url| DownloadTask {
                        inner: (url, self.cache.download_output_path(descriptor.name.clone())),
                        descriptor,
                    })
                    .map(SyncTask::from)
                    .with_context(|| format!("Manual action is required:\n\nURL: {url}\nGo to the website and download the file(s) manually"))
            }
        }
        .with_context(|| format!("when preparing download for\n{state:#?}"))
    }

    #[instrument(skip_all, fields(archives=%archives.len()))]
    pub async fn sync_downloads(self, archives: Vec<Archive>) -> TotalResult<WithArchiveDescriptor<PathBuf>> {
        let base_concurrency = 7;
        let sync_downloads = tracing::Span::current().tap(|pb| {
            pb.pb_set_length(archives.iter().map(|a| a.descriptor.size).sum());
            pb.pb_set_style(&io_progress_style());
        });

        futures::stream::iter(archives)
            .map(|Archive { descriptor, state }| async {
                match self
                    .cache
                    .clone()
                    .verify(descriptor.clone())
                    .instrument(sync_downloads.clone())
                    .pipe(tokio::task::spawn)
                    .map_context("task crashed")
                    .and_then(ready)
                    .await
                {
                    Ok(verified) => Ok(Either::Left(verified.tap(|verified| {
                        sync_downloads.pb_inc(verified.descriptor.size);
                        tracing::debug!(?verified, "succesfully verified a file");
                    }))),
                    Err(message) => self
                        .clone()
                        .prepare_sync_task(Archive {
                            descriptor: descriptor.tap(|descriptor| debug!(?descriptor, ?message, "could not verify a file, it will be downloaded")),
                            state,
                        })
                        .await
                        .map(Either::Right),
                }
            })
            .buffer_unordered(num_cpus::get())
            .collect::<Vec<_>>()
            .await
            .pipe(futures::stream::iter)
            .map_ok(|file| {
                let name = match &file {
                    Either::Left(left) => left.descriptor.name.clone(),
                    Either::Right(right) => match right {
                        SyncTask::MergeDownload(d) => d.descriptor.name.clone(),
                        SyncTask::Download(d) => d.descriptor.name.clone(),
                        SyncTask::Copy(d) => d.descriptor.name.clone(),
                    },
                };

                match file {
                    Either::Left(exists) => exists.pipe(Ok).pipe(ready).boxed(),
                    Either::Right(sync_task) => match sync_task {
                        SyncTask::MergeDownload(WithArchiveDescriptor { inner: (from, to), descriptor }) => {
                            stream_merge_file(from.clone(), to.clone(), descriptor.size)
                                .map_ok(|inner| WithArchiveDescriptor { inner, descriptor })
                                .map(move |res| res.with_context(|| format!("when downloading [{from:?} -> {to:?}]")))
                                .instrument(sync_downloads.clone())
                                .boxed()
                        }
                        SyncTask::Download(WithArchiveDescriptor { inner: (from, to), descriptor }) => stream_file(from.clone(), to.clone(), descriptor.size)
                            .map_ok(|inner| WithArchiveDescriptor { inner, descriptor })
                            .map(move |res| res.with_context(|| format!("when downloading [{from} -> {to:?}]")))
                            .instrument(sync_downloads.clone())
                            .boxed(),
                        SyncTask::Copy(WithArchiveDescriptor { inner: (from, to), descriptor }) => copy_local_file(from.clone(), to.clone(), descriptor.size)
                            .map_ok(|inner| WithArchiveDescriptor { inner, descriptor })
                            .map(move |res| res.with_context(|| format!("when when copying [{from:?} -> {to:?}]")))
                            .instrument(sync_downloads.clone())
                            .boxed(),
                    },
                }
                .inspect_err({
                    let name = name.clone();
                    move |message| tracing::debug!(?name, ?message)
                })
                .inspect_ok({
                    cloned![sync_downloads];
                    move |res| {
                        sync_downloads.pb_inc(res.descriptor.size);
                        tracing::debug!(name, "[OK]");
                    }
                })
                .pipe(tokio::task::spawn)
                .map_context("task crashed")
                .and_then(ready)
                .boxed()
            })
            .try_buffer_unordered(base_concurrency * 2)
            .multi_error_collect()
            .await
    }
}
