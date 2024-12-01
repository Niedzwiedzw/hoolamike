use {
    crate::{
        config_file::{HoolamikeConfig, InstallationConfig},
        helpers::human_readable_size,
        modlist_json::Modlist,
        progress_bars::{print_error, VALIDATE_TOTAL_PROGRESS_BAR},
    },
    anyhow::{Context, Result},
    downloads::Synchronizers,
    futures::{FutureExt, TryFutureExt},
    std::{future::ready, path::PathBuf},
    tap::prelude::*,
    tracing::info,
};

pub mod download_cache {
    use {
        crate::{
            downloaders::{helpers::FutureAnyhowExt, WithArchiveDescriptor},
            modlist_json::ArchiveDescriptor,
            progress_bars::{print_warn, vertical_progress_bar, PROGRESS_BAR, VALIDATE_TOTAL_PROGRESS_BAR},
        },
        anyhow::{Context, Result},
        futures::{FutureExt, TryFutureExt},
        std::{future::ready, hash::Hasher, path::PathBuf, sync::Arc},
        tap::prelude::*,
        tokio::io::AsyncReadExt,
    };

    #[derive(Debug, Clone)]
    pub struct DownloadCache {
        pub root_directory: PathBuf,
    }
    impl DownloadCache {
        pub fn new(root_directory: PathBuf) -> Result<Self> {
            std::fs::create_dir_all(&root_directory)
                .context("creating download directory")
                .map(|_| Self {
                    root_directory: root_directory.clone(),
                })
                .with_context(|| format!("creating download cache handler at [{}]", root_directory.display()))
        }
    }

    async fn read_file_size(path: &PathBuf) -> Result<u64> {
        tokio::fs::metadata(&path)
            .map_with_context(|| format!("getting size of {}", path.display()))
            .map_ok(|metadata| metadata.len())
            .await
    }
    async fn calculate_hash(path: PathBuf) -> Result<u64> {
        let file_name = path
            .file_name()
            .expect("file must have a name")
            .to_string_lossy()
            .to_string();
        let pb = PROGRESS_BAR
            .add(vertical_progress_bar(
                tokio::fs::metadata(&path).await?.len(),
                crate::progress_bars::ProgressKind::Validate,
            ))
            .tap_mut(|pb| {
                pb.set_message(file_name.clone());
            });

        let mut file = tokio::fs::File::open(&path)
            .map_with_context(|| format!("opening file [{}]", path.display()))
            .await?;
        let mut buffer: [u8; crate::BUFFER_SIZE] = std::array::from_fn(|_| 0);
        let mut hasher = xxhash_rust::xxh64::Xxh64::new(0);
        loop {
            match file.read(&mut buffer).await? {
                0 => break,
                read => {
                    pb.inc(read as u64);
                    VALIDATE_TOTAL_PROGRESS_BAR.inc(read as u64);
                    hasher.update(&buffer[..read]);
                }
            }
        }
        pb.finish_and_clear();
        Ok(hasher.finish())
    }

    fn to_base_64(input: &[u8]) -> String {
        use base64::prelude::*;
        BASE64_STANDARD.encode(input)
    }

    fn to_base_64_from_u64(input: u64) -> String {
        u64::to_ne_bytes(input).pipe(|bytes| to_base_64(&bytes))
    }

    pub async fn validate_hash(path: PathBuf, expected_hash: String) -> Result<PathBuf> {
        calculate_hash(path.clone())
            .map_ok(to_base_64_from_u64)
            .and_then(|hash| {
                hash.eq(&expected_hash)
                    .then_some(path.clone())
                    .with_context(|| format!("hash mismatch, expected [{expected_hash}], found [{hash}]"))
                    .pipe(ready)
            })
            .await
            .with_context(|| format!("validating hash for [{}]", path.display()))
    }

    async fn validate_file_size(path: PathBuf, expected_size: u64) -> Result<PathBuf> {
        read_file_size(&path).await.and_then(move |found_size| {
            found_size
                .eq(&expected_size)
                .then_some(path)
                .with_context(|| format!("size mismatch (expected [{expected_size} bytes], found [{found_size} bytes])"))
        })
    }

    impl DownloadCache {
        pub fn download_output_path(&self, file_name: String) -> PathBuf {
            self.root_directory.join(file_name)
        }
        pub async fn verify(self: Arc<Self>, descriptor: ArchiveDescriptor) -> Option<WithArchiveDescriptor<PathBuf>> {
            let ArchiveDescriptor { hash, meta: _, name, size } = descriptor.clone();
            self.download_output_path(name)
                .pipe(Ok)
                .pipe(ready)
                .and_then(|expected_path| async move {
                    tokio::fs::try_exists(&expected_path)
                        .map_with_context(|| format!("checking if path [{}] exists", expected_path.display()))
                        .map_ok(|exists| exists.then_some(expected_path.clone()))
                        .await
                })
                .and_then(|exists| match exists {
                    Some(existing_path) => validate_file_size(existing_path.clone(), size)
                        .and_then(|found_path| validate_hash(found_path, hash))
                        .map_ok(Some)
                        .boxed_local(),
                    None => None.pipe(Ok).pipe(ready).boxed_local(),
                })
                .await
                .and_then(|validated_path| {
                    validated_path
                        .context("does not exist")
                        .map(|inner| WithArchiveDescriptor {
                            inner,
                            descriptor: descriptor.clone(),
                        })
                })
                .tap_err(|message| print_warn(&descriptor.name, message))
                .ok()
        }
    }
}

pub mod downloads {
    use {
        super::*,
        crate::{
            config_file::{DownloadersConfig, GamesConfig},
            downloaders::{
                gamefile_source_downloader::{get_game_file_source_synchronizers, GameFileSourceSynchronizers},
                helpers::FutureAnyhowExt,
                nexus::{self, NexusDownloader},
                wabbajack_cdn::WabbajackCDNDownloader,
                CopyFileTask,
                DownloadTask,
                MergeDownloadTask,
                SyncTask,
                WithArchiveDescriptor,
            },
            modlist_json::{Archive, GoogleDriveState, HttpState, ManualState, NexusState, State},
            progress_bars::{print_error, vertical_progress_bar, ProgressKind, COPY_LOCAL_TOTAL_PROGRESS_BAR, DOWNLOAD_TOTAL_PROGRESS_BAR, PROGRESS_BAR},
            BUFFER_SIZE,
        },
        futures::{FutureExt, StreamExt, TryStreamExt},
        std::sync::Arc,
        tokio::io::{AsyncReadExt, BufReader, BufWriter},
        tracing::warn,
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
        pub config: Arc<DownloadersConfig>,
        inner: DownloadersInner,
        cache: Arc<download_cache::DownloadCache>,
        game_synchronizers: Arc<GameFileSourceSynchronizers>,
    }

    enum Either<L, R> {
        Left(L),
        Right(R),
    }

    async fn copy_local_file(from: PathBuf, to: PathBuf, expected_size: u64) -> Result<PathBuf> {
        let file_name = to
            .file_name()
            .expect("file must have a name")
            .to_string_lossy()
            .to_string();
        let pb = {
            COPY_LOCAL_TOTAL_PROGRESS_BAR.inc_length(expected_size);
            PROGRESS_BAR
                .add(vertical_progress_bar(expected_size, ProgressKind::Copy))
                .tap_mut(|pb| {
                    pb.set_message(file_name.clone());
                })
        };

        let mut target_file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&to)
            .map_with_context(|| format!("opening [{}]", to.display()))
            .await?;
        let mut source_file = tokio::fs::OpenOptions::new()
            .read(true)
            .open(&from)
            .map_with_context(|| format!("opening [{}]", from.display()))
            .await?;

        let mut writer = BufWriter::new(&mut target_file);
        let mut reader = BufReader::new(&mut source_file);

        let mut copied = 0;
        let mut buffer = [0; BUFFER_SIZE];
        loop {
            match reader.read(&mut buffer).await? {
                0 => break,
                copied_chunk => {
                    copied += copied_chunk as u64;
                    pb.inc(copied_chunk as u64);
                    COPY_LOCAL_TOTAL_PROGRESS_BAR.inc(copied_chunk as u64);
                    tokio::io::copy(&mut buffer.as_ref(), &mut writer)
                        .await
                        .with_context(|| format!("writing to {}", to.display()))?;
                }
            }
        }

        if copied != expected_size {
            anyhow::bail!(
                "[{from:?} -> {to:?}] local copy finished, but received unexpected size (expected [{expected_size}] bytes, downloaded [{copied} bytes])"
            )
        }
        pb.finish_and_clear();
        Ok(to)
    }

    pub async fn stream_merge_file(from: Vec<url::Url>, to: PathBuf, expected_size: u64) -> Result<PathBuf> {
        let file_name = to
            .file_name()
            .expect("file must have a name")
            .to_string_lossy()
            .to_string();
        let pb = {
            DOWNLOAD_TOTAL_PROGRESS_BAR.inc_length(expected_size);
            PROGRESS_BAR
                .add(vertical_progress_bar(expected_size, ProgressKind::Download))
                .tap_mut(|pb| {
                    pb.set_message(file_name.clone());
                })
        };

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&to)
            .map_with_context(|| format!("opening [{}]", to.display()))
            .await?;
        let mut writer = BufWriter::new(&mut file);
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
                        pb.inc(chunk.len() as u64);
                        DOWNLOAD_TOTAL_PROGRESS_BAR.inc(chunk.len() as u64);
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
        pb.finish_and_clear();
        Ok(to)
    }
    pub async fn stream_file(from: url::Url, to: PathBuf, expected_size: u64) -> Result<PathBuf> {
        let file_name = to
            .file_name()
            .expect("file must have a name")
            .to_string_lossy()
            .to_string();
        let pb = {
            DOWNLOAD_TOTAL_PROGRESS_BAR.inc_length(expected_size);
            PROGRESS_BAR
                .add(vertical_progress_bar(expected_size, ProgressKind::Download))
                .tap_mut(|pb| {
                    pb.set_message(file_name.clone());
                    pb.set_prefix("download");
                })
        };

        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&to)
            .map_with_context(|| format!("opening [{}]", to.display()))
            .await?;
        let mut writer = BufWriter::new(&mut file);
        let mut byte_stream = reqwest::get(from.to_string())
            .await
            .with_context(|| format!("making request to {from}"))?
            .bytes_stream();
        let mut downloaded = 0;
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(chunk) => {
                    downloaded += chunk.len() as u64;
                    pb.inc(chunk.len() as u64);
                    DOWNLOAD_TOTAL_PROGRESS_BAR.inc(chunk.len() as u64);
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
        pb.finish_and_clear();
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
            match state {
                State::Nexus(NexusState {
                    game_name, file_id, mod_id, ..
                }) => {
                    self.inner
                        .nexus
                        .clone()
                        .context("nexus not configured")
                        .pipe(ready)
                        .and_then(|nexus| {
                            nexus.download(nexus::DownloadFileRequest {
                                // TODO: validate this
                                game_domain_name: game_name,
                                mod_id,
                                file_id,
                            })
                        })
                        .await
                        .map(|url| DownloadTask {
                            inner: (url, self.cache.download_output_path(descriptor.name.clone())),
                            descriptor,
                        })
                        .map(SyncTask::from)
                }
                State::GoogleDrive(GoogleDriveState { id }) => crate::downloaders::google_drive::GoogleDriveDownloader::download(id, descriptor.size)
                    .await
                    .map(|url| DownloadTask {
                        inner: (url, self.cache.download_output_path(descriptor.name.clone())),
                        descriptor,
                    })
                    .map(SyncTask::Download),
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
                    .map(SyncTask::Copy),

                State::Http(HttpState { url, headers: _ }) => url
                    .pipe(|url| DownloadTask {
                        inner: (url, self.cache.download_output_path(descriptor.name.clone())),
                        descriptor,
                    })
                    .pipe(SyncTask::Download)
                    .pipe(Ok),
                State::Manual(ManualState { prompt, url }) => Err(anyhow::anyhow!("Manual action is required:\n\nURL: {url}\n{prompt}")),
                State::WabbajackCDN(state) => WabbajackCDNDownloader::prepare_download(state)
                    .await
                    .context("wabbajack... :)")
                    .map(|source_urls| MergeDownloadTask {
                        inner: (source_urls, self.cache.download_output_path(descriptor.name.clone())),
                        descriptor,
                    })
                    .map(SyncTask::MergeDownload),
            }
        }

        pub async fn sync_downloads(self, archives: Vec<Archive>) -> Vec<anyhow::Error> {
            futures::stream::iter(archives)
                .map(|Archive { descriptor, state }| async {
                    match self.cache.clone().verify(descriptor.clone()).await {
                        Some(verified) => Ok(Either::Left(verified.tap(|verified| info!(?verified, "succesfully verified a file")))),
                        None => self
                            .clone()
                            .prepare_sync_task(Archive {
                                descriptor: descriptor.tap(|descriptor| warn!(?descriptor, "could not verify a file, it will be downloaded")),
                                state,
                            })
                            .await
                            .map(Either::Right),
                    }
                })
                .buffer_unordered(num_cpus::get())
                .map_ok(|file| match file {
                    Either::Left(exists) => exists.pipe(Ok).pipe(ready).boxed_local(),
                    Either::Right(sync_task) => match sync_task {
                        SyncTask::MergeDownload(WithArchiveDescriptor { inner: (from, to), descriptor }) => {
                            stream_merge_file(from.clone(), to.clone(), descriptor.size)
                                .map_ok(|inner| WithArchiveDescriptor { inner, descriptor })
                                .map(move |res| res.with_context(|| format!("when downloading [{from:?} -> {to:?}]")))
                                .boxed_local()
                        }
                        SyncTask::Download(WithArchiveDescriptor { inner: (from, to), descriptor }) => stream_file(from.clone(), to.clone(), descriptor.size)
                            .map_ok(|inner| WithArchiveDescriptor { inner, descriptor })
                            .map(move |res| res.with_context(|| format!("when downloading [{from} -> {to:?}]")))
                            .boxed_local(),
                        SyncTask::Copy(WithArchiveDescriptor { inner: (from, to), descriptor }) => copy_local_file(from.clone(), to.clone(), descriptor.size)
                            .map_ok(|inner| WithArchiveDescriptor { inner, descriptor })
                            .map(move |res| res.with_context(|| format!("when when copying [{from:?} -> {to:?}]")))
                            .boxed_local(),
                    },
                })
                .try_buffer_unordered(10)
                .filter_map(|file| {
                    match file {
                        Ok(_) => None,
                        Err(error_occurred) => {
                            print_error("ERROR", &error_occurred);
                            Some(error_occurred)
                        }
                    }
                    .pipe(ready)
                })
                .collect::<Vec<_>>()
                .await
        }
    }
}

#[allow(clippy::needless_as_bytes)]
pub async fn install_modlist(
    HoolamikeConfig {
        downloaders,
        installation: InstallationConfig { modlist_file },
        games,
    }: HoolamikeConfig,
) -> Result<()> {
    let downloaders = Synchronizers::new(downloaders, games).context("setting up downloaders")?;

    modlist_file
        .context("no modlist file")
        .and_then(|modlist| {
            std::fs::read_to_string(&modlist)
                .with_context(|| format!("reading modlist at {}", modlist.display()))
                .tap_ok(|read| {
                    info!(
                        "modlist file {} read ({})",
                        modlist.display(),
                        human_readable_size(read.as_bytes().len() as u64)
                    )
                })
        })
        .and_then(|modlist| serde_json::from_str::<Modlist>(&modlist).context("parsing modlist"))
        .tap_ok(|modlist| {
            // PROGRESS
            modlist
                .archives
                .iter()
                .map(|archive| archive.descriptor.size)
                .sum::<u64>()
                .pipe(|total_size| {
                    VALIDATE_TOTAL_PROGRESS_BAR.set_length(total_size);
                })
        })
        .pipe(ready)
        .and_then(
            move |Modlist {
                      archives,
                      author: _,
                      description: _,
                      directives: _,
                      game_type: _,
                      image: _,
                      is_nsfw: _,
                      name: _,
                      readme: _,
                      version: _,
                      wabbajack_version: _,
                      website: _,
                  }| {
                downloaders
                    .sync_downloads(archives)
                    .map(|errors| match errors.as_slice() {
                        &[] => Ok(()),
                        many_errors => {
                            many_errors.iter().for_each(|error| {
                                print_error("ARCHIVE", error);
                            });
                            print_error("ARCHIVES", &anyhow::anyhow!("could not continue due to [{}] errors", many_errors.len()));
                            Err(errors.into_iter().next().unwrap())
                        }
                    })
            },
        )
        .await
}
