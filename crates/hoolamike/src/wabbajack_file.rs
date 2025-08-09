use {
    crate::{compression::ProcessArchive, install_modlist::directives::wabbajack_file_handle::WabbajackFileHandle, utils::PathReadWrite},
    anyhow::{Context, Result},
    std::{
        io::Read,
        path::{Path, PathBuf},
    },
    tap::prelude::*,
};

#[derive(Debug)]
#[allow(dead_code)]
pub struct WabbajackFile {
    pub wabbajack_file_path: PathBuf,
    pub wabbajack_entries: Vec<PathBuf>,
    pub modlist: super::modlist_json::Modlist,
}

const MODLIST_JSON_FILENAME: &str = "modlist";

impl WabbajackFile {
    #[tracing::instrument]
    pub fn load_wabbajack_file(at_path: PathBuf) -> Result<(WabbajackFileHandle, Self)> {
        at_path
            .open_file_read()
            .and_then(|(_, file)| crate::compression::compress_tools::ArchiveHandle::new(file))
            .context("reading archive")
            .and_then(|mut archive| {
                archive.list_paths().and_then(|entries| {
                    archive
                        .get_handle(Path::new(MODLIST_JSON_FILENAME))
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
                            wabbajack_file_path: at_path.clone(),
                            wabbajack_entries: entries,
                            modlist,
                        })
                        .and_then(|data| WabbajackFileHandle::from_archive(at_path).map(|archive| (archive, data)))
                })
            })
    }
}
