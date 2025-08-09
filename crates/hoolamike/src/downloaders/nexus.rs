use {
    super::helpers::{FutureAnyhowExt, ReqwestPrettyJsonResponse},
    crate::{
        modlist_json::{HumanUrl, NexusGameName, NexusState},
        nxm_handler::NxmDownloadLink,
    },
    anyhow::{Context, Result},
    chrono::{DateTime, Utc},
    futures::TryFutureExt,
    reqwest::{
        header::{HeaderMap, HeaderValue},
        Client,
        ClientBuilder,
        Response,
    },
    serde::{Deserialize, Serialize},
    std::{
        future::ready,
        iter::{empty, once},
        str::FromStr,
        sync::Arc,
    },
    tap::prelude::*,
};

pub struct NexusDownloader {
    client: Client,
}

const AUTH_HEADER: &str = "apikey";
const API_BASE_URL: &str = "https://api.nexusmods.com";
const WEBSITE_BASE_URL: &str = "https://www.nexusmods.com";

#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub struct DownloadFileRequest {
    pub game_domain_name: String,
    pub mod_id: usize,
    pub file_id: usize,
}

impl DownloadFileRequest {
    pub fn nexus_api_url(&self) -> String {
        self.pipe(
            |Self {
                 game_domain_name,
                 mod_id,
                 file_id,
             }| { format!("{API_BASE_URL}/v1/games/{game_domain_name}/mods/{mod_id}/files/{file_id}/download_link.json") },
        )
    }
    /// https://www.nexusmods.com/skyrimspecialedition/mods/141070
    pub fn nexus_website_url(&self) -> String {
        self.pipe(
            |Self {
                 game_domain_name,
                 mod_id,
                 file_id,
             }| {
                format!(
                    "{WEBSITE_BASE_URL}/{}/mods/{mod_id}?tab=files&file_id={file_id}&nmm=1",
                    game_domain_name.to_lowercase()
                )
            },
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadLinkResponse(Vec<NexusDownloadLink>);

#[derive(Debug)]
#[allow(dead_code)]
pub struct ThrottlingHeaders {
    /// X-RL-Hourly-Limit →100
    pub hourly_limit: usize,
    /// X-RL-Hourly-Remaining →96
    pub hourly_remaining: usize,
    /// X-RL-Hourly-Reset →2019-02-01T12:00:00+00:00
    pub hourly_reset: DateTime<Utc>,
    /// X-RL-Daily-Limit →2500
    pub daily_limit: usize,
    /// X-RL-Daily-Remaining →2488
    pub daily_remaining: usize,
    /// X-RL-Daily-Reset →2019-02-02 00:00:00 +0000
    pub daily_reset: DateTime<Utc>,
}

impl ThrottlingHeaders {
    fn from_response(response: &Response) -> Result<Self> {
        let headers = response.headers();
        fn header<E, T>(headers: &HeaderMap, header_name: &str) -> Result<T>
        where
            E: Send + Sync + std::fmt::Display + std::fmt::Debug + 'static,
            T: FromStr<Err = E> + Send + Sync + 'static,
        {
            headers
                .get(header_name)
                .context("no header")
                .and_then(|value| value.to_str().context("header is not a string"))
                .and_then(|value| {
                    value
                        .parse::<T>()
                        .map_err(|message| anyhow::anyhow!("{message:?}"))
                        .context("invalid type")
                })
                .with_context(|| format!("extracting [{header_name}] header"))
        }

        Ok(Self {
            hourly_limit: header(headers, "X-RL-Hourly-Limit")?,
            hourly_remaining: header(headers, "X-RL-Hourly-Remaining")?,
            hourly_reset: header(headers, "X-RL-Hourly-Reset")?,
            daily_limit: header(headers, "X-RL-Daily-Limit")?,
            daily_remaining: header(headers, "X-RL-Daily-Remaining")?,
            daily_reset: header(headers, "X-RL-Daily-Reset")?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexusDownloadLink {
    #[serde(rename = "URI")]
    pub uri: HumanUrl,
    pub name: String,
    pub short_name: String,
}

impl DownloadFileRequest {
    pub fn from_nexus_state(
        NexusState {
            game_name, file_id, mod_id, ..
        }: NexusState,
    ) -> Self {
        Self {
            // TODO: validate this
            game_domain_name: match game_name {
                NexusGameName::GameName(game_name) => game_name.to_string(),
                NexusGameName::Special(special) => match special {
                    crate::modlist_json::SpecialGameName::ModdingTools => "site".into(),
                    crate::modlist_json::SpecialGameName::FalloutNewVegas => "newvegas".into(),
                },
            },
            mod_id,
            file_id,
        }
    }
}

#[derive(derive_more::From, Debug)]
pub enum DownloadLinkKind {
    Premium(DownloadFileRequest),
    Free(NxmDownloadLink),
}

impl NexusDownloader {
    pub fn new(api_key: String) -> Result<Self> {
        empty()
            .chain(api_key.pipe(|api_key| (AUTH_HEADER, api_key)).pipe(once))
            .map(|(key, value)| {
                HeaderValue::from_str(&value)
                    .with_context(|| format!("invalid header value for {key}"))
                    .map(|value| (key, value))
            })
            .try_fold(HeaderMap::new(), |map, header| {
                header.map(|(key, value)| map.tap_mut(|map| map.insert(key, value).pipe(|_| ())))
            })
            .and_then(|headers| {
                ClientBuilder::new()
                    .default_headers(headers)
                    .build()
                    .context("building http client")
            })
            .map(|client| Self { client })
            .context("building NexusDownloader")
    }

    async fn generate_download_link(self: Arc<Self>, download_link: &DownloadLinkKind) -> Result<DownloadLinkResponse> {
        let (download_file_request, query_params) = match download_link {
            DownloadLinkKind::Premium(download_file_request) => (download_file_request, String::new()),
            DownloadLinkKind::Free(NxmDownloadLink { request, query }) => (
                request,
                serde_urlencoded::to_string(query)
                    .with_context(|| format!("Serializing query: {query:?}"))
                    .map(|q| format!("?{q}"))?,
            ),
        };
        let url = format!("{}{query_params}", download_file_request.nexus_api_url());
        self.client
            .get(&url)
            .send()
            .map_context("sending request")
            .inspect_ok(|response| {
                ThrottlingHeaders::from_response(response)
                    .tap_ok(|response| tracing::debug!("{response:?}"))
                    .pipe(|_| ())
            })
            .and_then(|response| response.json_response_ok(|_| Ok(())))
            .await
            .with_context(|| format!("when fetching from {url}"))
    }
    pub async fn download(self: Arc<Self>, request: impl Into<DownloadLinkKind>) -> Result<HumanUrl> {
        let request = request.into();
        self.clone()
            .generate_download_link(&request)
            .and_then(|download_link| {
                download_link
                    .0
                    .into_iter()
                    .next()
                    .context("no preferred download link found")
                    .pipe(ready)
            })
            .map_ok(|link| link.uri)
            .await
    }
}
