use {
    crate::{modlist_json::GameName, post_install_fixup::common::Resolution},
    anyhow::{Context, Result},
    indexmap::IndexMap,
    serde::{Deserialize, Serialize},
    std::{
        iter::{empty, once},
        path::{Path, PathBuf},
    },
    tap::prelude::*,
    tracing::{debug, info, warn},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct NexusConfig {
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, derivative::Derivative)]
#[derivative(Default)]
#[serde(deny_unknown_fields)]
pub struct DownloadersConfig {
    #[derivative(Default(value = "PathBuf::from(\"downloads\")"))]
    pub downloads_directory: PathBuf,
    pub nexus: NexusConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, derivative::Derivative)]
#[serde(deny_unknown_fields)]
pub struct GameConfig {
    pub root_directory: PathBuf,
}

fn join_default_path(segments: impl IntoIterator<Item = &'static str>) -> PathBuf {
    empty()
        .chain(once("FIXME"))
        .chain(segments)
        .fold(PathBuf::new(), |acc, next| acc.join(next))
}

#[derive(Debug, Clone, Serialize, Deserialize, derivative::Derivative)]
#[derivative(Default)]
#[serde(deny_unknown_fields)]
pub struct InstallationConfig {
    #[derivative(Default(value = "join_default_path([\"path\",\"to\",\"file.wabbajack\" ])"))]
    pub wabbajack_file_path: PathBuf,
    #[derivative(Default(value = "PathBuf::from(\"installed\")"))]
    pub installation_path: PathBuf,
}

pub type GamesConfig = IndexMap<GameName, GameConfig>;

fn default_games_config() -> GamesConfig {
    GamesConfig::new()
}

#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, derivative::Derivative)]
#[derivative(Default)]
#[serde(deny_unknown_fields)]
pub struct FixupConfig {
    #[derivative(Default(value = "Resolution {x: 1280, y: 800}"))]
    #[serde_as(as = "serde_with::DisplayFromStr")]
    pub game_resolution: Resolution,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ExtrasConfig {
    pub tale_of_two_wastelands: Option<crate::extensions::tale_of_two_wastelands_installer::ExtensionConfig>,
    pub texconv_wine: Option<crate::extensions::texconv_wine::ExtensionConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, derivative::Derivative)]
#[derivative(Default)]
#[serde(deny_unknown_fields)]
pub struct HoolamikeConfig {
    pub downloaders: DownloadersConfig,
    pub installation: InstallationConfig,
    #[derivative(Default(value = "default_games_config()"))]
    pub games: GamesConfig,
    pub fixup: Option<FixupConfig>,
    pub extras: Option<ExtrasConfig>,
}

pub static CONFIG_FILE_NAME: &str = "hoolamike.yaml";
impl HoolamikeConfig {
    pub fn write_with_gui_message(&self) -> Result<String> {
        self.pipe_ref(serde_yaml::to_string)
            .context("serialization failed")
            .map(|config| {
                format!(
                    "\n# {CONFIG_FILE_NAME} file, generated in gui with {} {} on {} \n# edit it according to your needs:\n{config}",
                    clap::crate_name!(),
                    clap::crate_version!(),
                    chrono::Utc::now().to_rfc3339()
                )
            })
    }
    pub fn write_default() -> Result<String> {
        Self::default()
            .pipe_ref(serde_yaml::to_string)
            .context("serialization failed")
            .map(|config| {
                format!(
                    "\n# default {CONFIG_FILE_NAME} file, generated using CLI interface with {} {} on {} \n# edit it according to your needs:\n{config}",
                    clap::crate_name!(),
                    clap::crate_version!(),
                    chrono::Utc::now().to_rfc3339()
                )
            })
    }
    pub fn read(path: &Path) -> Result<(PathBuf, Self)> {
        path.exists()
            .then(|| path.to_owned())
            .with_context(|| format!("config path [{}] does not exist", path.display()))
            .tap_ok(|config| info!("found config at '{}'", config.display()))
            .and_then(|config_path| {
                std::fs::read_to_string(&config_path)
                    .context("reading file")
                    .and_then(|config| serde_yaml::from_str::<Self>(&config).context("parsing config file"))
                    .map(|config| (config_path, config))
            })
            .with_context(|| format!("getting [{CONFIG_FILE_NAME}]"))
            .tap_err(|e| warn!("{e:?}"))
            .tap_ok(|config| {
                debug!("{config:?}");
            })
    }
}
