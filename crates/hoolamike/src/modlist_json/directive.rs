use super::*;

pub mod archive_hash_path;

pub mod create_bsa_directive;

pub use archive_hash_path::ArchiveHashPath;
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct FromArchiveDirective {
    /// hash: String
    /// Description: Hash of the file involved in the directive.
    /// Usage: Verify file integrity before processing.
    pub hash: String,
    /// size: u64
    /// Description: Size of the file.
    /// Usage: For validation and progress tracking.
    pub size: u64,
    /// to: String
    /// Description: Destination path for the directive's output.
    /// Usage: Where to place extracted or processed files.
    pub to: CaseInsensitivePathBuf,
    /// archive_hash_path: Option<Vec<String>>
    /// Description: Paths within an archive, identified by their hashes.
    /// Usage: Locate specific files inside archives.
    pub archive_hash_path: ArchiveHashPath,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct InlineFileDirective {
    /// hash: String
    /// Description: Hash of the file involved in the directive.
    /// Usage: Verify file integrity before processing.
    pub hash: String,
    /// size: u64
    /// Description: Size of the file.
    /// Usage: For validation and progress tracking.
    pub size: u64,
    #[serde(rename = "SourceDataID")]
    /// source_data_id: Option<String> (renamed from SourceDataID)
    /// Description: Identifier linking to the source data.
    /// Usage: May be used internally to reference data.
    pub source_data_id: uuid::Uuid,
    /// to: String
    /// Description: Destination path for the directive's output.
    /// Usage: Where to place extracted or processed files.
    pub to: CaseInsensitivePathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct PatchedFromArchiveDirective {
    /// hash: String
    /// Description: Hash of the file involved in the directive.
    /// Usage: Verify file integrity before processing.
    pub hash: String,
    /// size: u64
    /// Description: Size of the file.
    /// Usage: For validation and progress tracking.
    pub size: u64,
    /// to: String
    /// Description: Destination path for the directive's output.
    /// Usage: Where to place extracted or processed files.
    pub to: CaseInsensitivePathBuf,
    /// archive_hash_path: Option<Vec<String>>
    /// Description: Paths within an archive, identified by their hashes.
    /// Usage: Locate specific files inside archives.
    pub archive_hash_path: ArchiveHashPath,
    /// from_hash: Option<String>
    /// Description: Hash of the source file within an archive.
    /// Usage: Verify the correct source file is used.
    pub from_hash: String,
    #[serde(rename = "PatchID")]
    /// patch_id: Option<String> (renamed from PatchID)
    /// Description: Identifier for a patch operation.
    /// Usage: Apply the correct patch during installation.
    pub patch_id: uuid::Uuid,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct RemappedInlineFileDirective {
    /// hash: String
    /// Description: Hash of the file involved in the directive.
    /// Usage: Verify file integrity before processing.
    pub hash: String,
    /// size: u64
    /// Description: Size of the file.
    /// Usage: For validation and progress tracking.
    pub size: u64,
    #[serde(rename = "SourceDataID")]
    /// source_data_id: Option<String> (renamed from SourceDataID)
    /// Description: Identifier linking to the source data.
    /// Usage: May be used internally to reference data.
    pub source_data_id: uuid::Uuid,
    /// to: String
    /// Description: Destination path for the directive's output.
    /// Usage: Where to place extracted or processed files.
    pub to: CaseInsensitivePathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct TransformedTextureDirective {
    /// hash: String
    /// Description: Hash of the file involved in the directive.
    /// Usage: Verify file integrity before processing.
    pub hash: String,
    /// size: u64
    /// Description: Size of the file.
    /// Usage: For validation and progress tracking.
    pub size: u64,
    /// image_state: Option<ImageState>
    /// Description: Contains image-specific information if the directive deals with images.
    /// Usage: Process images correctly based on their properties.
    pub image_state: ImageState,
    /// to: String
    /// Description: Destination path for the directive's output.
    /// Usage: Where to place extracted or processed files.
    pub to: CaseInsensitivePathBuf,
    /// archive_hash_path: Option<Vec<String>>
    /// Description: Paths within an archive, identified by their hashes.
    /// Usage: Locate specific files inside archives.
    pub archive_hash_path: ArchiveHashPath,
}
