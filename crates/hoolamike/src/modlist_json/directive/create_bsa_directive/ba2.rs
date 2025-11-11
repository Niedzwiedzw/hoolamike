use {super::*, crate::serde_type_guard, case_insensitive_path::CaseInsensitivePathBuf, type_guard::WithTypeGuard};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct BA2DX10Entry {
    /// dir_hash: u64
    /// Description: Hash of the directory path.
    /// Usage: Verify file locations or detect conflicts.
    pub dir_hash: u32,
    /// chunk_hdr_len: Option<u64>
    /// Description: Length of the chunk header.
    /// Usage: Needed when processing files split into chunks.
    pub chunk_hdr_len: u64,
    /// chunks: Option<Vec<BA2DX10EntryChunk>>
    /// Description: List of chunks if the file is divided.
    /// Usage: Reassemble or process each chunk correctly.
    pub chunks: Vec<BA2DX10EntryChunk>,
    /// num_mips: Option<u64>
    /// Description: Number of mipmap levels in a texture.
    /// Usage: Important for texture processing.
    pub num_mips: u8,
    /// pixel_format: Option<u64>
    /// Description: Numeric code for the image's pixel format.
    /// Usage: Handle image data accurately.
    pub pixel_format: u8,
    /// tile_mode: Option<u64>
    /// Description: Tiling mode used in the texture.
    /// Usage: For rendering or processing textures.
    pub tile_mode: u8,
    #[serde(rename = "Unk8")]
    /// unk_8: Option<u8> (renamed from Unk8)
    /// Description: An unknown or unused field.
    /// Usage: May be ignored unless specified.
    pub unk_8: u8,
    /// extension: String
    /// Description: File extension (e.g., "dds", "nif").
    /// Usage: Determine how to process the file.
    pub extension: String,
    /// height: Option<u64>
    /// Description: Height of an image file.
    /// Usage: For image processing.
    pub height: u16,
    /// width: Option<u64>
    /// Description: Width of an image file.
    /// Usage: For image processing.
    pub width: u16,
    /// is_cube_map: Option<u8>
    /// Description: Indicates if the texture is a cube map.
    /// Usage: Special handling for cube maps in rendering.
    pub is_cube_map: u8,
    /// index: usize
    /// Description: Index of the file in a collection.
    /// Usage: Reference files in order.
    pub index: usize,
    /// name_hash: u64
    /// Description: Hash of the file name.
    /// Usage: Quickly compare or locate files.
    pub name_hash: u32,
    /// path: PathBuf
    /// Description: File system path to the file.
    /// Usage: Access the file during installation.
    pub path: CaseInsensitivePathBuf,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct BA2FileEntry {
    /// align: u64
    /// Description: Alignment requirement in bytes.
    /// Usage: Ensure correct alignment during file operations.
    pub align: u64,
    /// compressed: Option<bool>
    /// Description: Indicates if the file is compressed.
    /// Usage: Decompress if necessary during installation.
    pub compressed: bool,
    /// dir_hash: u64
    /// Description: Hash of the directory path.
    /// Usage: Verify file locations or detect conflicts.
    pub dir_hash: u32,
    /// extension: String
    /// Description: File extension (e.g., "dds", "nif").
    /// Usage: Determine how to process the file.
    pub extension: String,
    /// flags: u64
    /// Description: Additional flags for file properties.
    /// Usage: Influence processing based on flag values.
    pub flags: u64,
    /// index: usize
    /// Description: Index of the file in a collection.
    /// Usage: Reference files in order.
    pub index: usize,
    /// name_hash: u64
    /// Description: Hash of the file name.
    /// Usage: Quickly compare or locate files.
    pub name_hash: u32,
    /// path: PathBuf
    /// Description: File system path to the file.
    /// Usage: Access the file during installation.
    pub path: CaseInsensitivePathBuf,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize, enum_kinds::EnumKind)]
#[serde(tag = "$type")]
#[serde(deny_unknown_fields)]
#[enum_kind(BA2FileStateKind, derive(Serialize, Deserialize, PartialOrd, Ord, derive_more::Display,))]
pub enum FileState {
    #[serde(rename_all = "PascalCase")]
    BA2File(BA2FileEntry),
    BA2DX10Entry(BA2DX10Entry),
}

serde_type_guard!(BA2DirectiveStateGuard, "BA2State, Compression.BSA");

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct DirectiveStateData {
    /// has_name_table: bool
    /// Description: Indicates if the file contains a name table.
    /// Usage: Important for processing certain file formats.
    pub has_name_table: bool,
    /// header_magic: String
    /// Description: Magic number or signature in the file header.
    /// Usage: Verify file format before processing.
    pub header_magic: String,
    #[serde(rename = "Type")]
    /// kind: u64 (renamed from Type)
    /// Description: Numeric code representing the directive's kind.
    /// Usage: May influence processing logic.
    pub kind: u64,
    /// version: u64
    /// Description: Version number of the directive or file format.
    /// Usage: Ensure compatibility with processing routines.
    pub version: u64,
}

pub type DirectiveState = WithTypeGuard<DirectiveStateData, BA2DirectiveStateGuard>;
pub type Ba2 = create_bsa_directive::CreateBSADirectiveKind<ba2::DirectiveState, ba2::FileState>;
