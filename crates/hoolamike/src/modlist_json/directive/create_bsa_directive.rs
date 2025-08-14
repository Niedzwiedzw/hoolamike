use super::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "PascalCase")]
pub struct CreateBSADirectiveKind<DirectiveState, FileState> {
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
    pub to: MaybeWindowsPath,
    #[serde(rename = "TempID")]
    /// temp_id: Option<String> (renamed from TempID)
    /// Description: Temporary identifier used during processing.
    /// Usage: Track temporary files or operations.
    pub temp_id: String,
    /// file_states: Option<Vec<FileState>>
    /// Description: Details about the state of files involved in the directive.
    /// Usage: Handle files according to their specific properties.
    pub file_states: Vec<FileState>,
    /// state: Option<DirectiveState>
    /// Description: Additional metadata about the directive's state.
    /// Usage: Process directives accurately based on their state.
    pub state: DirectiveState,
}

pub mod ba2;
pub mod bsa;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum CreateBSADirective {
    Bsa(bsa::Bsa),
    Ba2(ba2::Ba2),
}

impl CreateBSADirective {
    pub fn size(&self) -> u64 {
        match self {
            CreateBSADirective::Bsa(d) => d.size,
            CreateBSADirective::Ba2(d) => d.size,
        }
    }
}
