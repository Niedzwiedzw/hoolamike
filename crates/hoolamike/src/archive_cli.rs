use {
    crate::{compression::ProcessArchive, utils::PathReadWrite},
    anyhow::{Context, Result},
    itertools::Itertools,
    std::path::PathBuf,
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
            ArchiveCliCommandInner::List { archive } => {
                crate::compression::ArchiveHandle::with_guessed(&archive, archive.extension(), |mut archive| archive.list_paths())
                    .map(|paths| paths.into_iter().for_each(|path| println!("{path:?}")))
            }
            ArchiveCliCommandInner::ExtractAll { archive } => crate::compression::ArchiveHandle::with_guessed(&archive, archive.extension(), |mut archive| {
                archive
                    .list_paths()
                    .and_then(|paths| archive.get_many_handles(paths.iter().map(|p| p.as_path()).collect_vec().as_slice()))
                    .and_then(|handles| {
                        handles.into_iter().try_for_each(|(path, mut handle)| {
                            path.open_file_write()
                                .and_then(|(_, mut file)| std::io::copy(&mut handle, &mut file).context("writing extracted file"))
                                .map(|size| info!(%size, "{path:?}"))
                        })
                    })
            }),
        }
    }
}
