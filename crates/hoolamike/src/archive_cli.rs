use {
    crate::{compression::ProcessArchive, path::PathBuf, utils::PathReadWrite},
    anyhow::{Context, Result},
    itertools::Itertools,
    tracing::info,
};

#[derive(clap::Args, Clone)]
pub struct ArchiveCliCommand {
    #[command(subcommand)]
    pub command: ArchiveCliCommandInner,
}

#[derive(clap::Subcommand, Clone)]
pub enum ArchiveCliCommandInner {
    List { archive: PathBuf },
    ExtractAll { archive: PathBuf },
}

impl ArchiveCliCommand {
    pub fn run(self) -> Result<()> {
        match self.command {
            ArchiveCliCommandInner::List { archive } => archive.try_exists().and_then(|archive| {
                crate::compression::ArchiveHandle::with_guessed(&archive, archive.as_path().extension(), |mut archive| archive.list_paths())
                    .map(|paths| paths.into_iter().for_each(|path| println!("{path:?}")))
            }),
            ArchiveCliCommandInner::ExtractAll { archive } => archive.try_exists().and_then(|archive| {
                crate::compression::ArchiveHandle::with_guessed(&archive, archive.as_path().extension(), |mut archive| {
                    archive
                        .list_paths()
                        .and_then(|paths| archive.get_many_handles(paths.iter().collect_vec().as_slice()))
                        .and_then(|handles| {
                            handles.into_iter().try_for_each(|(path, mut handle)| {
                                path.as_path()
                                    .open_file_write()
                                    .and_then(|(_, mut file)| std::io::copy(&mut handle, &mut file).context("writing extracted file"))
                                    .map(|size| info!(%size, "{path:?}"))
                            })
                        })
                })
            }),
        }
    }
}
