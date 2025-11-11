use {
    super::*,
    crate::{
        modlist_json::directive::RemappedInlineFileDirective,
        progress_bars_v2::IndicatifWrapIoExt,
        utils::{ExistingPathRead, PathReadWrite, StreamLenExt},
    },
    std::io::Read,
    tracing::instrument,
    typed_path::Utf8PlatformPath,
    wabbajack_file_handle::WabbajackFileHandle,
};

#[allow(dead_code)]
pub mod wabbajack_consts {

    use typed_path::Utf8TypedPath;

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
        pub(crate)  static SETTINGS_INI: Utf8TypedPath<'static> = Utf8TypedPath::unix("settings.ini");
        pub(crate)  static MO2_MOD_FOLDER_NAME:  Utf8TypedPath<'static> = Utf8TypedPath::unix("mods");
        pub(crate)  static MO2_PROFILES_FOLDER_NAME:  Utf8TypedPath<'static> = Utf8TypedPath::unix("profiles");
        pub(crate)  static BSA_CREATION_DIR:  Utf8TypedPath<'static> = Utf8TypedPath::unix("TEMP_BSA_FILES");
        pub(crate)  static KNOWN_MODIFIED_FILES: [ Utf8TypedPath<'static>; 2] = [Utf8TypedPath::unix("modlist.txt"), Utf8TypedPath::unix("SkyrimPrefs.ini")];
    }

    pub(crate) const STEP_PREPARING: &str = "Preparing";
    pub(crate) const STEP_INSTALLING: &str = "Installing";
    pub(crate) const STEP_DOWNLOADING: &str = "Downloading";
    pub(crate) const STEP_HASHING: &str = "Hashing";
    pub(crate) const STEP_FINISHED: &str = "Finished";
}

#[derive(Debug)]
pub struct RemappingContext {
    pub game_folder: ExistingPathBuf,
    pub output_directory: ExistingPathBuf,
    pub downloads_directory: ExistingPathBuf,
}

#[extension_traits::extension(trait PathCrossPlatformJoineryExt)]
impl Utf8PlatformPath {
    fn join_with_delimiter(&self, delimiter: &str) -> String {
        self.iter().join(delimiter)
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
                        .as_path()
                        .join_with_delimiter(delimiter)
                        .pipe_as_ref(trim_relative_path_start)
                };
                let install_directory = |delimiter| {
                    install_directory
                        .as_path()
                        .join_with_delimiter(delimiter)
                        .pipe_as_ref(trim_relative_path_start)
                };
                let downloads_directory = |delimiter| {
                    downloads_directory
                        .as_path()
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
                    .case_insensitive()
                    .join_case_insensitive(to.clone())
                    .and_then(|to| to.as_path().open_file_write())
                    .and_then(|(_, mut file)| {
                        std::io::copy(&mut tracing::Span::current().wrap_read(size, std::io::Cursor::new(output)), &mut file).context("writing remapped file")
                    })
            })
    }
}
