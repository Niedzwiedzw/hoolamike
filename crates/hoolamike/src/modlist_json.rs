use {
    crate::{install_modlist::download_cache::to_base_64_from_u64, utils::MaybeWindowsPath},
    serde::{Deserialize, Serialize},
    std::hash::Hasher,
    tap::prelude::*,
};

#[macro_export]
macro_rules! test_example {
    ($input:expr, $name:ident, $ty:ty) => {
        #[test]
        fn $name() -> anyhow::Result<()> {
            use anyhow::Context;
            serde_json::from_str::<$ty>($input)
                .with_context(|| format!("{}\ncould not be parsed as {}", $input, std::any::type_name::<$ty>()))
                .map(|_| ())
        }
    };
}

#[derive(
    derive_more::FromStr,
    derive_more::Display,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    derive_more::AsRef,
    derive_more::From,
    derive_more::Into,
    serde::Serialize,
    serde::Deserialize,
    derive_more::AsMut,
)]
pub struct HumanUrl(url::Url);

impl std::fmt::Debug for HumanUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Url({self})")
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Modlist {
    /// archives: Vec<Archive>
    /// Description: A list of archives (mod files) required for the modlist.
    /// Usage: You'll need to download each archive listed here.
    pub archives: Vec<Archive>,
    /// author: String
    /// Description: The name of the modlist's creator.
    /// Usage: Display or record the author's name for attribution.
    #[serde(default)]
    pub author: String,
    /// description: String
    /// Description: A brief description of the modlist.
    /// Usage: Show this to users to inform them about the modlist.
    #[serde(default)]
    pub description: String,
    /// directives: Vec<Directive>
    /// Description: Instructions on how to process the archives and install the mods.
    /// Usage: Follow these directives to install the mods correctly.
    pub directives: Vec<Directive>,
    /// game_type: String
    /// Description: The type of game the modlist is for (e.g., "Skyrim", "Fallout4").
    /// Usage: Ensure compatibility with the user's game.
    pub game_type: GameName,
    /// image: String
    /// Description: Path or URL to an image representing the modlist.
    /// Usage: Display this image in your tool's UI.
    #[serde(default)]
    pub image: String,
    /// is_nsfw: bool
    /// Description: Indicates if the modlist contains adult content.
    /// Usage: Warn users or enforce age restrictions as necessary.
    #[serde(rename = "IsNSFW")]
    pub is_nsfw: bool,
    /// name: String
    /// Description: The name of the modlist.
    /// Usage: Display or record the modlist's name.
    pub name: String,
    /// readme: String
    /// Description: Path or URL to a README file with detailed instructions.
    /// Usage: Provide access to the README for additional guidance.
    #[serde(default)]
    pub readme: String,
    /// version: String
    /// Description: The version number of the modlist.
    /// Usage: Manage updates or compatibility checks.
    pub version: String,
    /// wabbajack_version: String
    /// Description: The version of Wabbajack used to create the modlist.
    /// Usage: Ensure compatibility with your tool.
    pub wabbajack_version: String,
    /// website: String
    /// Description: The modlist's website or homepage.
    /// Usage: Provide users with a link for more information.
    #[serde(default)]
    pub website: String,
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Eq, PartialOrd, Ord, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct ArchiveDescriptor {
    /// hash: String
    /// Description: A hash (e.g., SHA256) of the archive file for integrity verification.
    /// Usage: Verify downloaded files to prevent corruption or tampering.
    pub hash: String,
    /// meta: String
    /// Description: Metadata about the archive, possibly including download source info.
    /// Usage: May contain details needed for downloading or processing the archive.
    pub meta: String,
    /// name: String
    /// Description: The filename of the archive.
    /// Usage: Use this when saving or referencing the archive.
    pub name: String,
    /// size: u64
    /// Description: Size of the archive in bytes.
    /// Usage: For progress tracking and verifying download completeness.
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Archive {
    #[serde(flatten)]
    pub descriptor: ArchiveDescriptor,
    /// state: State
    /// Description: Contains information about where and how to download the archive.
    /// Usage: Use the State fields to handle the download process.
    pub state: State,
}

pub mod type_guard;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize, enum_kinds::EnumKind, Clone)]
#[serde(tag = "$type")]
#[serde(deny_unknown_fields)]
#[enum_kind(DownloadKind, derive(Serialize, Deserialize, PartialOrd, Ord, derive_more::Display,))]
pub enum State {
    #[serde(rename = "NexusDownloader, Wabbajack.Lib")]
    Nexus(NexusState),
    #[serde(rename = "GameFileSourceDownloader, Wabbajack.Lib")]
    GameFileSource(GameFileSourceState),
    #[serde(rename = "MegaDownloader, Wabbajack.Lib")]
    Mega(MegaState),
    #[serde(rename = "GoogleDriveDownloader, Wabbajack.Lib")]
    GoogleDrive(GoogleDriveState),
    #[serde(rename = "MediaFireDownloader+State, Wabbajack.Lib")]
    MediaFire(MediaFireState),
    #[serde(rename = "HttpDownloader, Wabbajack.Lib")]
    Http(HttpState),
    #[serde(rename = "ManualDownloader, Wabbajack.Lib")]
    Manual(ManualState),
    #[serde(rename = "WabbajackCDNDownloader+State, Wabbajack.Lib")]
    WabbajackCDN(WabbajackCDNDownloaderState),
}

impl State {
    pub fn kind(&self) -> DownloadKind {
        DownloadKind::from(self)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct HttpState {
    #[serde(default)]
    pub headers: Vec<()>,
    pub url: HumanUrl,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct ManualState {
    pub prompt: String,
    pub url: HumanUrl,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct WabbajackCDNDownloaderState {
    pub url: HumanUrl,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct GoogleDriveState {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct MediaFireState {
    pub url: HumanUrl,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct MegaState {
    pub url: HumanUrl,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct GameFileSourceState {
    pub game_version: String,
    pub hash: String,
    pub game_file: MaybeWindowsPath,
    pub game: GameName,
}

#[derive(Debug, Serialize, Deserialize, Clone, derive_more::Display, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Constructor)]
pub struct GameName(String);

#[derive(Debug, Serialize, Deserialize, Clone, derive_more::Display, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpecialGameName {
    ModdingTools,
    FalloutNewVegas,
}

#[derive(Debug, Serialize, Deserialize, Clone, derive_more::Display, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(untagged)]
pub enum NexusGameName {
    Special(SpecialGameName),
    GameName(GameName),
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
#[serde(deny_unknown_fields)]
pub struct NexusState {
    pub game_name: NexusGameName,
    #[serde(rename = "FileID")]
    pub file_id: usize,
    #[serde(rename = "ModID")]
    pub mod_id: usize,
    pub author: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "ImageURL")]
    /// image_url: Option<String>
    /// Description: URL to an image associated with the mod.
    /// Usage: Display in your tool's UI.
    pub image_url: Option<String>,
    #[serde(rename = "IsNSFW")]
    /// is_nsfw: Option<bool> (renamed from IsNSFW)
    /// Description: Indicates if the mod contains adult content.
    /// Usage: Implement content warnings or filters.
    pub is_nsfw: bool,
    /// name: Option<String>
    /// Description: The name of the mod or archive.
    /// Usage: Display to the user or use in logs.
    pub name: String,
    /// version: Option<String>
    /// Description: The version of the mod.
    /// Usage: Ensure correct versions are downloaded.
    pub version: String,
}

pub mod directive;

#[derive(Debug, Serialize, Deserialize, enum_kinds::EnumKind)]
#[serde(tag = "$type")]
#[serde(deny_unknown_fields)]
#[enum_kind(DirectiveKind, derive(Serialize, Deserialize, PartialOrd, Ord, derive_more::Display, Hash, clap::ValueEnum))]
pub enum Directive {
    CreateBSA(directive::create_bsa_directive::CreateBSADirective),
    FromArchive(directive::FromArchiveDirective),
    InlineFile(directive::InlineFileDirective),
    PatchedFromArchive(directive::PatchedFromArchiveDirective),
    RemappedInlineFile(directive::RemappedInlineFileDirective),
    TransformedTexture(directive::TransformedTextureDirective),
}

impl Directive {
    pub fn size(&self) -> u64 {
        match self {
            Directive::CreateBSA(d) => d.size(),
            Directive::FromArchive(d) => d.size,
            Directive::InlineFile(d) => d.size,
            Directive::PatchedFromArchive(d) => d.size,
            Directive::RemappedInlineFile(d) => d.size,
            Directive::TransformedTexture(d) => d.size,
        }
    }
    pub fn directive_hash(&self) -> String {
        serde_json::to_string(self).unwrap().pipe(|out| {
            let mut hasher = xxhash_rust::xxh64::Xxh64::new(0);
            hasher.update(out.as_bytes());
            hasher.finish().pipe(to_base_64_from_u64)
        })
    }
}

impl Directive {
    pub fn directive_kind(&self) -> DirectiveKind {
        DirectiveKind::from(self)
    }
}

pub mod image_format;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct ImageState {
    /// format: String
    /// Description: Image file format (e.g., "DDS", "PNG").
    /// Usage: Handle the image appropriately during installation.
    pub format: self::image_format::DXGIFormat,
    /// height: u64
    /// Description: Height of the image in pixels.
    /// Usage: May be needed for processing or validation.
    pub height: u32,
    /// mip_levels: u64
    /// Description: Number of mipmap levels in the image.
    /// Usage: Important for rendering and performance.
    pub mip_levels: u32,
    /// perceptual_hash: String
    /// Description: Hash representing the image's visual content.
    /// Usage: Detect duplicate or similar images.
    pub perceptual_hash: String,
    /// width: u64
    /// Description: Width of the image in pixels.
    /// Usage: May be needed for processing or validation.
    pub width: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct BA2DX10EntryChunk {
    /// align: u64
    /// Description: Alignment requirement for the chunk.
    /// Usage: Ensure correct alignment when reassembling.
    pub align: u64,
    /// compressed: bool
    /// Description: Indicates if the chunk is compressed.
    /// Usage: Decompress as needed.
    pub compressed: bool,
    /// end_mip: u64
    /// Description: Ending mipmap level for this chunk.
    /// Usage: For texture processing.
    pub end_mip: u64,
    /// full_sz: u64
    /// Description: Full size of the chunk in bytes.
    /// Usage: For progress tracking and validation.
    pub full_sz: u64,
    /// start_mip: u64
    /// Description: Starting mipmap level for this chunk.
    /// Usage: For texture processing.
    pub start_mip: u64,
}

pub mod parsing_helpers {
    use {
        anyhow::{Context, Result},
        itertools::Itertools,
        serde_json::Value,
        std::collections::BTreeMap,
        tap::prelude::*,
        tracing::info,
    };

    #[allow(dead_code)]
    #[derive(Debug)]
    enum ValueSummary<'a> {
        Map { fields: BTreeMap<&'a str, Self> },
        Array { first_element: Option<Box<Self>>, len: usize },
        Other(&'a serde_json::Value),
    }

    pub fn validate_modlist_file(input: &str) -> Result<()> {
        input
            .tap(|input| {
                info!("file is {} bytes long", input.len());
            })
            .pipe_as_ref(serde_json::from_str::<Value>)
            .context("bad json")
            .and_then(|node| serde_json::to_string_pretty(&node).context("serializing"))
            .and_then(move |pretty_input| {
                serde_json::from_str::<crate::modlist_json::Modlist>(&pretty_input)
                    .pipe(|res| match res.as_ref() {
                        Ok(_) => res.context(""),
                        Err(e) => e.line().pipe(|line| {
                            res.with_context(|| {
                                pretty_input
                                    .lines()
                                    .enumerate()
                                    .map(|(idx, line)| format!("{}. {line}", idx + 1))
                                    .skip(line - 20)
                                    .take(40)
                                    .join("\n")
                            })
                        }),
                    })
                    .context("bad modlist")
            })
            .map(|_| ())
    }

    #[allow(unexpected_cfgs)]
    #[cfg(test)]
    mod ad_hoc_test {
        use super::*;

        #[allow(dead_code)]
        fn summarize_node(node: &Value) -> ValueSummary<'_> {
            match node {
                Value::Array(vec) => ValueSummary::Array {
                    first_element: vec.first().map(summarize_node).map(Box::new),
                    len: vec.len(),
                },
                Value::Object(map) => ValueSummary::Map {
                    fields: map
                        .iter()
                        .map(|(key, value)| (key.as_str(), summarize_node(value)))
                        .collect(),
                },
                other => ValueSummary::Other(other),
            }
        }

        #[cfg(ignore)]
        // #[ignore]
        #[test_log::test]
        fn test_wasteland_reborn() -> anyhow::Result<()> {
            use super::*;

            include_str!("../../../playground/dupa/modlist").pipe(validate_modlist_file)
        }
    }
}
