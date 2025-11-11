use {
    super::helpers::FutureAnyhowExt,
    crate::{
        config_file::{GameConfig, GamesConfig},
        install_modlist::download_cache::validate_hash_wabbajack,
        modlist_json::{GameFileSourceState, GameName},
    },
    anyhow::{Context, Result},
    case_insensitive_path::{ExistingPathBuf, PathExistsUtf8Ext},
    futures::TryFutureExt,
    indexmap::IndexMap,
    std::{future::ready, path::PathBuf},
    tap::prelude::*,
};

pub struct GameFileSourceDownloader {
    game_name: GameName,
    source_directory: ExistingPathBuf,
}

impl GameFileSourceDownloader {
    pub fn new(game_name: GameName, GameConfig { root_directory }: GameConfig) -> Result<Self> {
        root_directory
            .exists_utf8()
            .map(|source_directory| Self { source_directory, game_name })
    }
    pub async fn prepare_copy(
        &self,
        GameFileSourceState {
            game_version: _,
            hash,
            game_file,
            game,
        }: GameFileSourceState,
    ) -> Result<ExistingPathBuf> {
        self.game_name
            .eq(&game)
            .then_some(())
            .with_context(|| format!("expected downloader for [{game}], but this is a downloader for [{}]", self.game_name))
            .map(|_| game_file)
            .pipe(ready)
            .and_then(|game_file| {
                self.source_directory
                    .clone()
                    .case_insensitive()
                    .join_case_insensitive(game_file)
                    .pipe(ready)
                    .and_then(async |game_file| game_file.try_exists_async().await)
            })
            .and_then(|source| validate_hash_wabbajack(source, hash))
            .await
    }
}

pub type GameFileSourceSynchronizers = IndexMap<GameName, GameFileSourceDownloader>;

pub fn get_game_file_source_synchronizers(config: GamesConfig) -> Result<GameFileSourceSynchronizers> {
    config
        .into_iter()
        .map(|(game, config)| {
            GameFileSourceDownloader::new(game.clone(), config)
                .with_context(|| format!("creating copy manager for [{game}]"))
                .map(|downloader| (game, downloader))
        })
        .collect::<Result<_>>()
        .context("instantiating game downloaders, check config")
}
