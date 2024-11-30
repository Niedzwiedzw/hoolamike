use std::{future::ready, path::PathBuf};

use anyhow::{Context, Result};
use downloads::Downloaders;
use futures::TryFutureExt;
use tracing::info;

use crate::{
    config_file::{HoolamikeConfig, InstallationConfig},
    helpers::human_readable_size,
    modlist_json::Modlist,
};
use tap::prelude::*;

pub mod download_cache {
    use anyhow::{Context, Result};
    use base64::Engine;
    use futures::{FutureExt, TryFutureExt};
    use std::{future::ready, hash::Hasher, path::PathBuf, sync::Arc};
    use tap::prelude::*;
    use tokio::io::AsyncReadExt;
    use tracing::{info, warn};

    use crate::{
        downloaders::{helpers::FutureAnyhowExt, WithArchiveDescriptor},
        modlist_json::ArchiveDescriptor,
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
                .with_context(|| {
                    format!(
                        "creating download cache handler at [{}]",
                        root_directory.display()
                    )
                })
        }
    }

    async fn read_file_size(path: &PathBuf) -> Result<u64> {
        tokio::fs::metadata(&path)
            .map_with_context(|| format!("getting size of {}", path.display()))
            .map_ok(|metadata| metadata.len())
            .await
    }
    async fn calculate_hash(path: PathBuf) -> Result<u64> {
        let mut file = tokio::fs::File::open(&path)
            .map_with_context(|| format!("opening file [{}]", path.display()))
            .await?;
        let mut buffer: [u8; 1024] = std::array::from_fn(|_| 0);
        let mut hasher = xxhash_rust::xxh64::Xxh64::new(0);
        loop {
            match file.read(&mut buffer).await? {
                0 => break,
                read => {
                    hasher.update(&buffer[..read]);
                }
            }
        }
        Ok(hasher.finish())
    }

    fn to_base_64(input: &[u8]) -> String {
        use base64::prelude::*;
        BASE64_STANDARD.encode(input)
    }

    fn to_base_64_from_u64(input: u64) -> String {
        u64::to_ne_bytes(input).pipe(|bytes| to_base_64(&bytes))
    }

    async fn validate_hash(path: PathBuf, expected_hash: String) -> Result<PathBuf> {
        calculate_hash(path.clone())
            .map_ok(to_base_64_from_u64)
            .and_then(|hash| {
                hash.eq(&expected_hash)
                    .then_some(path.clone())
                    .with_context(|| {
                        format!("hash mismatch, expected [{expected_hash}], found [{hash}]")
                    })
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
                .context("size mismatch (expected {size}, found {found_size})")
        })
    }

    impl DownloadCache {
        pub fn download_output_path(&self, file_name: String) -> PathBuf {
            self.root_directory.join(file_name)
        }
        pub async fn verify(
            self: Arc<Self>,
            descriptor: ArchiveDescriptor,
        ) -> Option<WithArchiveDescriptor<PathBuf>> {
            let ArchiveDescriptor {
                hash,
                meta: _,
                name,
                size,
            } = descriptor.clone();
            self.download_output_path(name)
                .pipe(Ok)
                .pipe(ready)
                .and_then(|expected_path| async move {
                    tokio::fs::try_exists(&expected_path)
                        .map_with_context(|| {
                            format!("checking if path [{}] exists", expected_path.display())
                        })
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
                        .map(|inner| WithArchiveDescriptor { inner, descriptor })
                })
                .tap_err(|message| warn!("could not validate file, redownloading [{message}]"))
                .ok()
        }
    }
}

pub mod downloads {
    use std::{os::fd::AsFd, sync::Arc};

    use futures::{FutureExt, StreamExt, TryStreamExt};
    use tokio::sync::RwLock;
    use tracing::{debug, error, warn};

    use super::*;
    use crate::{
        config_file::DownloadersConfig,
        downloaders::{
            helpers::FutureAnyhowExt,
            nexus::{self, NexusDownloader},
            DownloadTask, WithArchiveDescriptor,
        },
        modlist_json::{Archive, ArchiveDescriptor, DownloadKind, NexusState, State, UnknownState},
    };

    #[derive(Clone)]
    pub struct DownloadersInner {
        pub nexus: Option<Arc<NexusDownloader>>,
    }

    impl DownloadersInner {
        pub fn new(
            DownloadersConfig {
                nexus,
                downloads_directory: _,
            }: DownloadersConfig,
        ) -> Result<Self> {
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
    pub struct Downloaders {
        config: Arc<DownloadersConfig>,
        inner: DownloadersInner,
        cache: Arc<download_cache::DownloadCache>,
    }

    enum Either<L, R> {
        Left(L),
        Right(R),
    }

    async fn stream_file(from: url::Url, to: PathBuf) -> Result<PathBuf> {
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&to)
            .map_with_context(|| format!("opening [{}]", to.display()))
            .await?;
        let mut byte_stream = reqwest::get(from.to_string())
            .await
            .with_context(|| format!("making request to {from}"))?
            .bytes_stream();
        while let Some(chunk) = byte_stream.next().await {
            tokio::io::copy(&mut chunk?.as_ref(), &mut file)
                .await
                .with_context(|| format!("writing to fd {:?}", file.as_fd()))?;
        }
        Ok(to)
    }

    impl Downloaders {
        pub fn new(config: DownloadersConfig) -> Result<Self> {
            Ok(Self {
                config: Arc::new(config.clone()),
                cache: Arc::new(
                    download_cache::DownloadCache::new(config.downloads_directory.clone())
                        .context("building download cache")?,
                ),
                inner: DownloadersInner::new(config).context("building downloaders")?,
            })
        }

        pub async fn prepare_download_archive(
            self,
            Archive { descriptor, state }: Archive,
        ) -> Result<DownloadTask> {
            // debug!(
            //     ?game,
            //     ?version,
            //     ?id,
            //     ?kind,
            //     ?image_url,
            //     ?url,
            //     ?author,
            //     ?mod_id,
            //     ?name,
            //     ?state_name,
            //     ?size,
            //     "downloading archive"
            // );
            match state {
                State::Nexus(NexusState {
                    game_name,
                    file_id,
                    mod_id,
                    ..
                }) => {
                    self.inner
                        .nexus
                        .clone()
                        .context("nexus not configured")
                        .pipe(ready)
                        .and_then(|nexus| {
                            nexus.download(nexus::DownloadFileRequest {
                                // TODO: validate this
                                game_domain_name: game_name.to_lowercase(),
                                mod_id,
                                file_id,
                            })
                        })
                        .map_ok(|url| DownloadTask {
                            inner: (
                                url,
                                self.cache.download_output_path(descriptor.name.clone()),
                            ),
                            descriptor,
                        })
                        .await
                }
                State::GameFileSource(kind) => Err(anyhow::anyhow!("{kind:?} is not implemented")),
                State::GoogleDrive(kind) => Err(anyhow::anyhow!("{kind:?} is not implemented")),
                State::Http(kind) => Err(anyhow::anyhow!("{kind:?} is not implemented")),
                State::Manual(kind) => Err(anyhow::anyhow!("{kind:?} is not implemented")),
                State::WabbajackCDN(kind) => Err(anyhow::anyhow!("{kind:?} is not implemented")),
            }
        }

        pub async fn sync_downloads(self, archives: Vec<Archive>) -> Result<()> {
            futures::stream::iter(archives)
                .map(|Archive { descriptor, state }| async {
                    match self.cache.clone().verify(descriptor.clone()).await {
                        Some(verified) => {
                            Ok(Either::Left(verified.tap(|verified| {
                                info!(?verified, "succesfully verified a file")
                            })))
                        }
                        None => self
                            .clone()
                            .prepare_download_archive(Archive {
                                descriptor: descriptor.tap(|descriptor| {
                                    warn!(
                                        ?descriptor,
                                        "could not verify a file, it will be downloaded"
                                    )
                                }),
                                state,
                            })
                            .await
                            .map(Either::Right),
                    }
                })
                .buffer_unordered(num_cpus::get())
                .map_ok(|file| match file {
                    Either::Left(exists) => exists.pipe(Ok).pipe(ready).boxed_local(),
                    Either::Right(WithArchiveDescriptor {
                        inner: (from, to),
                        descriptor,
                    }) => stream_file(from, to)
                        .map_ok(|inner| WithArchiveDescriptor { inner, descriptor })
                        .boxed_local(),
                })
                .try_buffer_unordered(10)
                .for_each_concurrent(10, |file| {
                    match file {
                        Ok(all_good) => info!(?all_good),
                        Err(error_occurred) => error!(?error_occurred),
                    }
                    .pipe(ready)
                })
                .await
                .pipe(Ok)
        }
    }
}

#[allow(clippy::needless_as_bytes)]
pub async fn install_modlist(
    HoolamikeConfig {
        downloaders,
        installation:
            InstallationConfig {
                modlist_file,
                original_game_dir,
                installation_dir,
            },
    }: HoolamikeConfig,
) -> Result<()> {
    let downloaders = Downloaders::new(downloaders).context("setting up downloaders")?;

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
        .pipe(ready)
        .and_then(
            move |Modlist {
                      archives,
                      author,
                      description,
                      directives,
                      game_type,
                      image,
                      is_nsfw,
                      name,
                      readme,
                      version,
                      wabbajack_version,
                      website,
                  }| { downloaders.sync_downloads(archives) },
        )
        .await
}
