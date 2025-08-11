pub mod fallout_new_vegas_4gb_patch;
pub mod tale_of_two_wastelands_installer;
pub mod texconv_proton {
    use {
        serde::{Deserialize, Serialize},
        std::path::PathBuf,
    };

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct ExtensionConfig {
        pub proton_path: PathBuf,
        pub prefix_dir: PathBuf,
        pub steam_path: PathBuf,
        pub texconv_path: PathBuf,
    }
}
