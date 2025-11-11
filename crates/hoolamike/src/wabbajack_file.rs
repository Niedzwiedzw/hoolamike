use {
    crate::{compression::ProcessArchive, install_modlist::directives::wabbajack_file_handle::WabbajackFileHandle, utils::ExistingPathRead},
    anyhow::{Context, Result},
    case_insensitive_path::{CaseInsensitivePathBuf, ExistingPath, ExistingPathBuf},
    std::{
        io::Read, str::FromStr,
    },
    tap::prelude::*,
};

#[derive(Debug)]
#[allow(dead_code)]
pub struct WabbajackFile {
    pub wabbajack_file_path: ExistingPathBuf,
    pub wabbajack_entries: Vec<CaseInsensitivePathBuf>,
    pub modlist: super::modlist_json::Modlist,
}

const MODLIST_JSON_FILENAME: &str = "modlist";

impl WabbajackFile {
    #[tracing::instrument]
    pub fn load_modlist_json(at_path: &ExistingPath) -> Result<Self> {
        at_path
            .open_file_read()
            .and_then(|(_, file)| crate::compression::compress_tools::ArchiveHandle::new(file))
            .context("reading archive")
            .and_then(|mut archive| {
                archive.list_paths().and_then(|entries| {
                    archive
                        .get_handle(&MODLIST_JSON_FILENAME.pipe(CaseInsensitivePathBuf::from_str).expect("bad modlist json filename"))
                        .context("looking up file by name")
                        .and_then(|mut handle| {
                            String::new()
                                .pipe(|mut out| handle.read_to_string(&mut out).map(|_| out))
                                .context("reading modlist json to string")
                        })
                        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).context("reading archive json contents"))
                        .and_then(|json| {
                            serde_json::to_string_pretty(&json)
                                .context("serializing json")
                                .and_then(|output| serde_json::from_str(&output).context("output is a valid json but not a valid modlist file"))
                        })
                        .with_context(|| format!("reading [{MODLIST_JSON_FILENAME}]"))
                        .map(|modlist| Self {
                            wabbajack_file_path: at_path.to_owned(),
                            wabbajack_entries: entries,
                            modlist,
                        })
                })
            })
    }
    #[tracing::instrument]
    pub fn load_wabbajack_file(at_path: &ExistingPath) -> Result<(WabbajackFileHandle, Self)> {
        Self::load_modlist_json(at_path).and_then(|data| WabbajackFileHandle::from_archive(at_path).map(|archive| (archive, data)))
    }
}
