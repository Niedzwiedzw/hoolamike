use {
    crate::{
        config_file::{HoolamikeConfig, InstallationConfig},
        downloaders::{
            nexus::{DownloadFileRequest, NexusDownloader},
            DownloadTask,
            WithArchiveDescriptor,
        },
        install_modlist::{
            download_cache::DownloadCache,
            downloads::{stream_file, HTTP_CLIENT},
        },
        modlist_json::{Archive, HumanUrl, Modlist, State},
        progress_bars_v2::io_progress_style,
        utils::{spawn_rayon, Obfuscated},
        wabbajack_file::WabbajackFile,
    },
    anyhow::{anyhow, Context, Result},
    cli::HandleNxmCli,
    futures::{FutureExt, StreamExt, TryFutureExt, TryStreamExt},
    indicatif::ProgressBar,
    itertools::Itertools,
    notify::{event::CreateKind, Watcher},
    serde::{Deserialize, Serialize},
    single_instance_server::listen_for_nxm_links,
    std::{collections::HashMap, convert::identity, future::ready, path::PathBuf, sync::Arc},
    tap::prelude::*,
    tokio_stream::wrappers::UnboundedReceiverStream,
    tracing::{debug, info, warn},
    tracing_indicatif::span_ext::IndicatifSpanExt,
    utils::AbortOnDropExt,
};

pub mod cli;
pub mod register;
pub mod utils;

pub async fn handle_nxm_link(port: u16, nxm_link: HumanUrl) -> Result<()> {
    HTTP_CLIENT
        .post(single_instance_server::server_address(port).pipe(|address| format!("http://{address}")))
        .json(&single_instance_server::Message::NewNxm(nxm_link))
        .send()
        .map(|r| r.context("sending request"))
        .and_then(|r| r.error_for_status().context("bad status").pipe(ready))
        .and_then(|r| r.text().map(|r| r.context("reading text")))
        .await
        .context("sending request failed")
        .map(|response| info!("response: {response}"))
        .tap_err(|message| {
            tracing::error!("{message:?}");
            std::thread::sleep(std::time::Duration::from_secs(3));
        })
}

#[derive(Debug, Clone)]
pub struct NxmDownloadLink {
    pub request: DownloadFileRequest,
    pub query: NxmQuery,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct NxmQuery {
    #[serde(skip_serializing)]
    #[allow(dead_code)]
    pub user_id: Obfuscated<u64>,
    pub key: Obfuscated<String>,
    pub expires: u64,
}

impl NxmDownloadLink {
    pub fn parse_url(url: HumanUrl) -> Result<Self> {
        url.to_string()
            .pipe_deref(|url| {
                url.split_once("nxm://")
                    .context("no nxm://")
                    .and_then(|(_, uri)| {
                        uri.split_once("?")
                            .context("no query?")
                            .and_then(|(path, query)| {
                                path.split("/")
                                    .collect_vec()
                                    .try_conv::<[&str; 5]>()
                                    .map_err(|e| anyhow!("bad params: '{}'", e.join("/")))
                                    .context("bad path length")
                                    .and_then(|path| match path {
                                        [game_domain_name, "mods", mod_id, "files", file_id] => Ok(DownloadFileRequest {
                                            game_domain_name: game_domain_name.to_string(),
                                            mod_id: mod_id
                                                .parse()
                                                .with_context(|| format!("bad mod_id: {mod_id}"))?,
                                            file_id: file_id
                                                .parse()
                                                .with_context(|| format!("bad file_id: {file_id}"))?,
                                        }),
                                        e => Err(anyhow!("bad params: '{}'", e.join("/"))),
                                    })
                                    .and_then(|request| {
                                        serde_urlencoded::from_str::<NxmQuery>(query)
                                            .with_context(|| format!("bad query: [{query}]"))
                                            .map(|query| NxmDownloadLink { request, query })
                                    })
                            })
                    })
            })
            .with_context(|| format!("bad url: '{url}'"))
    }
}

pub async fn run(
    HoolamikeConfig {
        downloaders,
        installation: InstallationConfig {
            wabbajack_file_path,
            installation_path: _,
        },
        games: _,
        fixup: _,
        extras: _,
    }: HoolamikeConfig,
    HandleNxmCli {
        port,
        nxm_link,
        skip_nxm_register,
        use_browser,
    }: HandleNxmCli,
) -> Result<()> {
    match nxm_link {
        Some(nxm_link) => handle_nxm_link(port, nxm_link).await,
        None => {
            if !skip_nxm_register {
                self::register::register_nxm_handler().context("setting up nxm didn't work")?;
                info!("nxm is set up");
            }
            info!("starting to listen for nxm links");

            let nexus_downloader = downloaders
                .nexus
                .api_key
                .clone()
                .context("nexus api key is required even for non-premium users")
                .and_then(|api_key| {
                    NexusDownloader::new(api_key)
                        .map(Arc::new)
                        .context("bad nexus client")
                })
                .context("nxm handling will not work wihout nexus working")?;
            let (
                _wabbajack_file_handle,
                WabbajackFile {
                    wabbajack_file_path: _,
                    wabbajack_entries: _,
                    modlist: Modlist { archives, .. },
                },
            ) = spawn_rayon(move || WabbajackFile::load_wabbajack_file(wabbajack_file_path))
                .await
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
                .context("extracting specified wabbajack file")?;

            let download_cache = DownloadCache::new(downloaders.downloads_directory)
                .context("initializing download cache")
                .map(Arc::new)?;

            let mut archive_lookup = {
                let archives_pb = ProgressBar::new(archives.len() as _);
                archives
                    .pipe(futures::stream::iter)
                    .pipe(|archives| archives_pb.wrap_stream(archives))
                    .filter_map(|Archive { descriptor, state }| match state {
                        State::Nexus(nexus_state) => Some(WithArchiveDescriptor {
                            descriptor,
                            inner: nexus_state,
                        })
                        .pipe(ready),
                        _ => ready(None),
                    })
                    .map({
                        cloned![download_cache];
                        move |archive| {
                            cloned![download_cache];
                            async move {
                                download_cache
                                    .clone()
                                    .verify(archive.descriptor.clone())
                                    .map(move |result| (result, archive))
                                    .await
                            }
                        }
                    })
                    .buffered(num_cpus::get())
                    .filter_map(|(validated, archive)| match validated {
                        Ok(skip) => {
                            info!("skipping {}", skip.descriptor.name);
                            ready(None)
                        }
                        Err(reason) => {
                            info!("needs redownload: {} (reason:\n{reason:?}\n)", archive.descriptor.name);
                            ready(Some(archive))
                        }
                    })
                    .map(|archive| {
                        (
                            archive
                                .inner
                                .clone()
                                .pipe(DownloadFileRequest::from_nexus_state)
                                .nexus_website_url(),
                            archive,
                        )
                    })
                    .collect::<HashMap<_, _>>()
                    .await
            };

            let (downloads_task, queue_download_task) = {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DownloadTask>();
                tokio::task::spawn(async move {
                    UnboundedReceiverStream::new(rx)
                        .map(
                            |DownloadTask {
                                 inner: (url, output_path),
                                 descriptor,
                             }| {
                                stream_file(url.clone(), output_path.clone(), descriptor.size)
                                    .inspect_err(move |reason| tracing::error!(?url, ?output_path, "could not finish download:\n\n{reason:?}"))
                            },
                        )
                        .buffer_unordered(8)
                        .try_for_each(|e| {
                            info!("[OK] {e:?}");
                            ready(Ok(()))
                        })
                        .await
                })
                .abort_on_drop()
                .pipe(|task| (task, tx))
            };

            let (filesystem_changes, _guard) = {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let mut watcher =
                    notify::RecommendedWatcher::new(move |res| tx.send(res).unwrap(), notify::Config::default()).context("watching the filesystem failed")?;
                watcher
                    .watch(&download_cache.root_directory, notify::RecursiveMode::NonRecursive)
                    .context("watching downloads directory for changes")?;
                (UnboundedReceiverStream::new(rx), watcher)
            };

            let nxm_clicks = listen_for_nxm_links(port)
                .filter_map(|event| match event {
                    single_instance_server::ServerEvent::Message(message) => message.pipe(anyhow::Ok).pipe(Some).pipe(ready),
                    single_instance_server::ServerEvent::Listener(ev) => match ev {
                        Ok(_) => Err(anyhow!("server stopped??")).pipe(Some).pipe(ready),
                        Err(reason) => {
                            warn!(?reason, "bad event");
                            ready(None)
                        }
                    },
                })
                .map_ok(|message| match message {
                    single_instance_server::Message::NewNxm(human_url) => human_url,
                })
                .and_then(|url| NxmDownloadLink::parse_url(url).pipe(ready))
                .and_then(move |request| {
                    info!("new nxm request: {request:?}");
                    nexus_downloader
                        .clone()
                        .download(request.clone())
                        .map_ok(|url| (url, request.request))
                })
                .filter_map(|data| match data {
                    Ok(data) => ready(Some(data)),
                    Err(message) => {
                        warn!("something went wrong: {message:?}");
                        ready(None)
                    }
                })
                .boxed();

            let new_files = filesystem_changes
                .filter_map(|e| match e {
                    Ok(event) => match event.kind {
                        notify::EventKind::Create(CreateKind::File) => ready(event.paths.into_iter().next()),
                        _ => ready(None),
                    },
                    Err(message) => {
                        tracing::error!(?message, "watching filesysstem is failing");
                        ready(None)
                    }
                })
                .boxed();

            let initial_count = archive_lookup.len();

            #[derive(derive_more::From)]
            enum DownloaderEvent {
                NxmClick((HumanUrl, DownloadFileRequest)),
                Newfile(PathBuf),
            }

            let mut downloader_events = [nxm_clicks.map(DownloaderEvent::from).boxed(), new_files.map(DownloaderEvent::from).boxed()]
                .pipe(futures::stream::iter)
                .flatten_unordered(100);

            let mut filename_lookup = archive_lookup
                .values()
                .map(|archive| {
                    (
                        download_cache.download_output_path(archive.descriptor.name.clone()),
                        DownloadFileRequest::from_nexus_state(archive.inner.clone()).nexus_website_url(),
                    )
                })
                .collect::<HashMap<_, _>>();

            while let Some(nexus_website_url) = archive_lookup.keys().next().cloned() {
                info!("queued {}/{}", initial_count.saturating_sub(archive_lookup.len()), initial_count);

                info!("opening {nexus_website_url} with {use_browser}");
                tokio::process::Command::new(&use_browser)
                    .arg(&nexus_website_url)
                    .output()
                    .await
                    .context("spawning browser process")
                    .and_then(|o| {
                        o.status
                            .success()
                            .then_some(())
                            .ok_or(o.status)
                            .map_err(|s| anyhow!("bad status: {s}"))
                    })
                    .with_context(|| format!("opening [{nexus_website_url}] with ({use_browser}) failed: check 'hoolamike handle-nxm --help'"))?;

                match downloader_events.next().await {
                    Some(s) => match s {
                        DownloaderEvent::NxmClick((download_url, click)) => {
                            let Some(archive) = archive_lookup.remove(&click.nexus_website_url()) else {
                                warn!("not on the list: {click:?}");
                                continue;
                            };
                            queue_download_task
                                .send(DownloadTask {
                                    inner: (download_url, download_cache.download_output_path(archive.descriptor.name.clone())),
                                    descriptor: archive.descriptor,
                                })
                                .with_context(|| format!("when queueing download task for {}", archive.inner.name))?;
                        }
                        DownloaderEvent::Newfile(path_buf) => filename_lookup
                            .remove(&path_buf)
                            .context("unexpected path")
                            .and_then(|nexus| {
                                archive_lookup
                                    .remove(&nexus)
                                    .with_context(|| format!("no [{path_buf:?}] in nexus queue"))
                            })
                            .with_context(|| format!("removing from queue because of (presumed manual download) of {path_buf:?}"))
                            .pipe(|r| match r {
                                Ok(removed) => info!("manual download detected: {} ({path_buf:?})", removed.descriptor.name),
                                Err(message) => {
                                    debug!("looks like you were trying to download something manually at {path_buf:?}:\n{message:?}")
                                }
                            }),
                    },
                    None => {
                        anyhow::bail!("server stopped?")
                    }
                }
            }
            info!("You have queued all the files, awaiting for all downloads to finish");
            downloads_task
                .await
                .context("download task has crashed")
                .and_then(identity)?;
            info!("All nexus links from modlists downloaded, you can now proceed with standard installation (nexus links will only get validated)");

            Ok(())
        }
    }
}

pub mod single_instance_server {
    use {
        crate::modlist_json::HumanUrl,
        anyhow::{Context, Result},
        axum::{
            extract::State,
            response::{Html, IntoResponse},
            routing::post,
            Json,
            Router,
        },
        futures::{FutureExt, Stream, StreamExt, TryFutureExt},
        reqwest::StatusCode,
        serde::{Deserialize, Serialize},
        std::net::{Ipv4Addr, SocketAddr},
        tap::prelude::*,
        tracing::{info, trace},
    };

    pub const DEFAULT_PORT: u16 = 8007;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub enum Message {
        NewNxm(HumanUrl),
    }

    struct NxmApiError(anyhow::Error);

    type NxmApiResult<T> = std::result::Result<T, NxmApiError>;

    impl IntoResponse for NxmApiError {
        fn into_response(self) -> axum::response::Response {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!("<h1>Something isn't hoola:</h1><p>{:?}</p>", self.0)),
            )
                .into_response()
        }
    }

    pub type Sender = tokio::sync::mpsc::Sender<Message>;
    pub type Receiver = tokio::sync::mpsc::Receiver<Message>;

    pub fn create_channels() -> (Sender, Receiver) {
        tokio::sync::mpsc::channel(9000)
    }
    pub(super) fn server_address(port: u16) -> SocketAddr {
        SocketAddr::new(Ipv4Addr::new(127, 0, 0, 1).into(), port)
    }
    async fn run_server(tx: Sender, port: u16) -> Result<()> {
        let address = server_address(port);
        info!("starting the server on {address}");
        Router::new()
            .route("/", post(handler))
            .with_state(tx)
            .pipe(|handler| {
                tokio::net::TcpListener::bind(address)
                    .map(|r| r.with_context(|| format!("binding to {address:?}")))
                    .inspect_ok(|listener| info!("listening on [{:?}]", listener.local_addr().ok()))
                    .and_then(async move |listener| {
                        axum::serve(listener, handler)
                            .await
                            .context("spawning the server")
                    })
            })
            .await
    }

    #[derive(Debug)]
    pub enum ServerEvent {
        Message(Message),
        Listener(Result<()>),
    }
    pub fn listen_for_nxm_links(port: u16) -> impl Stream<Item = ServerEvent> {
        let (tx, rx) = create_channels();
        [
            tokio_stream::wrappers::ReceiverStream::new(rx)
                .map(ServerEvent::Message)
                .boxed(),
            run_server(tx, port)
                .into_stream()
                .map(ServerEvent::Listener)
                .boxed(),
        ]
        .pipe(futures::stream::iter)
        .flatten_unordered(3)
    }

    async fn handler(State(tx): State<Sender>, Json(message): Json<Message>) -> NxmApiResult<Html<&'static str>> {
        trace!("new message: {message:#?}");
        tx.send(message)
            .await
            .context("communicating to channel failed")
            .map_err(NxmApiError)
            .map(|_| Html("<h1>Hoolamike says: roger that!</h1>"))
    }
}
