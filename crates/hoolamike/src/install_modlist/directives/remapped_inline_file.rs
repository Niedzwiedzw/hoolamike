use {
    super::*,
    crate::{
        modlist_json::directive::RemappedInlineFileDirective,
        progress_bars_v2::IndicatifWrapIoExt,
        utils::{spawn_rayon, PathReadWrite},
    },
    std::io::{Read, Seek},
    tracing::instrument,
    wabbajack_file_handle::WabbajackFileHandle,
};

#[allow(dead_code)]
pub mod wabbajack_consts {
    use std::path::Path;

    pub(crate) static GAME_PATH_MAGIC_BACK: &str = "{--||GAME_PATH_MAGIC_BACK||--}";
    pub(crate) static GAME_PATH_MAGIC_DOUBLE_BACK: &str = "{--||GAME_PATH_MAGIC_DOUBLE_BACK||--}";
    pub(crate) static GAME_PATH_MAGIC_FORWARD: &str = "{--||GAME_PATH_MAGIC_FORWARD||--}";

    pub(crate) static MO2_PATH_MAGIC_BACK: &str = "{--||MO2_PATH_MAGIC_BACK||--}";
    pub(crate) static MO2_PATH_MAGIC_DOUBLE_BACK: &str = "{--||MO2_PATH_MAGIC_DOUBLE_BACK||--}";
    pub(crate) static MO2_PATH_MAGIC_FORWARD: &str = "{--||MO2_PATH_MAGIC_FORWARD||--}";

    pub(crate) static DOWNLOAD_PATH_MAGIC_BACK: &str = "{--||DOWNLOAD_PATH_MAGIC_BACK||--}";
    pub(crate) static DOWNLOAD_PATH_MAGIC_DOUBLE_BACK: &str = "{--||DOWNLOAD_PATH_MAGIC_DOUBLE_BACK||--}";
    pub(crate) static DOWNLOAD_PATH_MAGIC_FORWARD: &str = "{--||DOWNLOAD_PATH_MAGIC_FORWARD||--}";
    thread_local! {
        pub(crate)  static SETTINGS_INI: &'static Path = Path::new("settings.ini");
        pub(crate)  static MO2_MOD_FOLDER_NAME: &'static Path = Path::new("mods");
        pub(crate)  static MO2_PROFILES_FOLDER_NAME: &'static Path = Path::new("profiles");
        pub(crate)  static BSA_CREATION_DIR: &'static Path = Path::new("TEMP_BSA_FILES");
        pub(crate)  static KNOWN_MODIFIED_FILES: [&'static Path; 2] = [Path::new("modlist.txt"), Path::new("SkyrimPrefs.ini")];
    }

    pub(crate) const STEP_PREPARING: &str = "Preparing";
    pub(crate) const STEP_INSTALLING: &str = "Installing";
    pub(crate) const STEP_DOWNLOADING: &str = "Downloading";
    pub(crate) const STEP_HASHING: &str = "Hashing";
    pub(crate) const STEP_FINISHED: &str = "Finished";
}

#[derive(Debug)]
pub struct RemappingContext {
    pub game_folder: PathBuf,
    pub output_directory: PathBuf,
    pub downloads_directory: PathBuf,
}

#[extension_traits::extension(trait PathCrossPlatformJoineryExt)]
impl Path {
    fn join_with_delimiter(&self, delimiter: &str) -> String {
        self.iter().map(|e| e.to_string_lossy()).join(delimiter)
    }
}

impl RemappingContext {
    pub fn remap_file_contents(&self, data: &str) -> String {
        self.pipe(
            |Self {
                 game_folder,
                 output_directory: install_directory,
                 downloads_directory,
             }| {
                fn trim_relative_path_start(path: &str) -> String {
                    path.trim_start_matches(r#".\\"#)
                        .trim_start_matches(r#".\"#)
                        .trim_start_matches(r#"./"#)
                        .to_string()
                }
                let game_folder = |delimiter| {
                    game_folder
                        .join_with_delimiter(delimiter)
                        .pipe_as_ref(trim_relative_path_start)
                };
                let install_directory = |delimiter| {
                    install_directory
                        .join_with_delimiter(delimiter)
                        .pipe_as_ref(trim_relative_path_start)
                };
                let downloads_directory = |delimiter| {
                    downloads_directory
                        .join_with_delimiter(delimiter)
                        .pipe_as_ref(trim_relative_path_start)
                };

                const BACK: &str = r#"\"#;
                const DOUBLE_BACK: &str = r#"\\"#;
                const FORWARD: &str = r#"/"#;
                data.replace(wabbajack_consts::GAME_PATH_MAGIC_BACK, game_folder(BACK).as_str())
                    .replace(wabbajack_consts::GAME_PATH_MAGIC_DOUBLE_BACK, game_folder(DOUBLE_BACK).as_str())
                    .replace(wabbajack_consts::GAME_PATH_MAGIC_FORWARD, game_folder(FORWARD).as_str())
                    .replace(wabbajack_consts::MO2_PATH_MAGIC_BACK, install_directory(BACK).as_str())
                    .replace(wabbajack_consts::MO2_PATH_MAGIC_DOUBLE_BACK, install_directory(DOUBLE_BACK).as_str())
                    .replace(wabbajack_consts::MO2_PATH_MAGIC_FORWARD, install_directory(FORWARD).as_str())
                    .replace(wabbajack_consts::DOWNLOAD_PATH_MAGIC_BACK, downloads_directory(BACK).as_str())
                    .replace(wabbajack_consts::DOWNLOAD_PATH_MAGIC_DOUBLE_BACK, downloads_directory(DOUBLE_BACK).as_str())
                    .replace(wabbajack_consts::DOWNLOAD_PATH_MAGIC_FORWARD, downloads_directory(FORWARD).as_str())
                    .tap(|new| tracing::trace!("remapped:\n{data}-->\n{new}"))
            },
        )
    }
}

#[derive(Clone, Debug)]
pub struct RemappedInlineFileHandler {
    pub remapping_context: Arc<RemappingContext>,
    pub wabbajack_file: WabbajackFileHandle,
}

impl RemappedInlineFileHandler {
    #[instrument]
    pub fn handle(
        self,
        RemappedInlineFileDirective {
            hash,
            size,
            source_data_id,
            to,
        }: RemappedInlineFileDirective,
    ) -> Result<u64> {
        let Self {
            remapping_context,
            wabbajack_file,
        } = self;
        wabbajack_file
            .get_source_data(source_data_id)
            .and_then(|source_data| {
                source_data
                    .open_file_read()
                    .map(|(_, file)| (source_data, file))
            })
            .context("reading the file for remapping")
            .and_then(|(_guard, mut handle)| {
                String::new().pipe(|mut out| {
                    tracing::Span::current()
                        .wrap_read(handle.stream_len().context("reading file size ")?, handle)
                        .read_to_string(&mut out)
                        .context("extracting file for remapping")
                        .map(|_| out)
                })
            })
            .map(|file| remapping_context.remap_file_contents(&file))
            .and_then(|output| {
                remapping_context
                    .output_directory
                    .join(to.clone().into_path())
                    .open_file_write()
                    .and_then(|(_, mut file)| {
                        std::io::copy(&mut tracing::Span::current().wrap_read(size, std::io::Cursor::new(output)), &mut file).context("writing remapped file")
                    })
            })
    }
}
