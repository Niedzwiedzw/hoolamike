use {
    crate::modlist_json::HumanUrl,
    anyhow::{Context, Result},
    serde::{Deserialize, Serialize},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct Variable {
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: u8,
    #[serde(default)]
    pub exclude_delimiter: bool,
    pub value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct Location {
    pub name: String,
    #[serde(rename = "Type")]
    pub kind: u8,
    pub value: String,
    #[serde(default)]
    pub create_folder: bool,
    pub archive_type: Option<u16>,
    pub archive_flags: Option<u16>,
    pub files_flags: Option<u16>,
    pub archive_compressed: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct Tag {
    pub name: String,
    #[serde(rename = "ID")]
    pub id: u16,
    pub text_color: String,
    pub back_color: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct Check {
    #[serde(rename = "Type")]
    pub kind: u8,
    pub inverted: bool,
    pub loc: u8,
    pub file: String,
    pub custom_message: String,
    pub checksums: Option<String>,
    pub free_size: Option<u64>,
}

#[derive(Debug, serde_repr::Serialize_repr, serde_repr::Deserialize_repr, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum AssetRawKind {
    Copy = 0,
    New = 1,
    Patch = 2,
    XwmaFuz = 3,
    OggEnc2 = 4,
    AudioEnc = 5,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum AssetRaw {
    A(u16, AssetRawKind, String, u8, u8, u8, String),
    B(u16, AssetRawKind, String, u8, u8, u8, String, String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct FileAttr {
    pub value: String,
    pub last_modified: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct PostCommand {
    pub value: String,
    pub wait: bool,
    pub hidden: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DebugAndRelease<T>((Vec<T>, Vec<T>));

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct Gui {
    pub files: String,
    pub width: u32,
    pub height: u32,
    pub borderless: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct Package {
    pub title: String,
    pub version: String,
    pub author: String,
    pub home_page: HumanUrl,
    pub description: String,
    #[serde(rename = "GUI")]
    pub gui: Gui,
}

/// Tale of two Wastelands installer manifest file
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct Manifest {
    pub package: Package,
    pub variables: DebugAndRelease<Variable>,
    pub locations: DebugAndRelease<Location>,
    pub tags: Vec<Tag>,
    pub checks: Vec<Check>,
    pub file_attrs: Vec<FileAttr>,
    pub post_commands: Vec<PostCommand>,
    pub assets: Vec<AssetRaw>,
}

#[test]
fn test_ad_hoc_example_manifest_file() -> Result<()> {
    let example = include_str!("../../../../../playground/begin-again/ttw-installer/ttw-mpi-extracted/_package/index.json");
    serde_json::from_str::<serde_json::Value>(example)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .and_then(|example| serde_json::from_str::<Manifest>(&example))
        .context("bad json")
        .map(|_| ())
}
