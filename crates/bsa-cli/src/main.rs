#![allow(future_incompatible)]

use {
    anyhow::{Context, Result},
    ba2::{ByteSlice, Reader, fo4::FileWriteOptions},
    clap::{Parser, Subcommand},
    std::path::{Path, PathBuf},
    tap::prelude::*,
};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// command to execute
    #[command(subcommand)]
    command: ArchiveCommand,
}

#[derive(Subcommand)]
enum ArchiveCommand {
    /// list the archive under path
    List {
        /// path to archive
        archive_path: PathBuf,
    },
    /// extract file to current directory
    Extract {
        /// path to archive
        archive_path: PathBuf,
        /// path to file within archive
        file_path: MaybeWindowsPath,
    },
}
fn list_paths_with_originals<'a>(archive: &ba2::fo4::Archive<'a>) -> Vec<(MaybeWindowsPath, ba2::fo4::ArchiveKey<'a>)> {
    archive
        .iter()
        .map(|(key, _file)| {
            key.name()
                .pipe(|s| s.as_bytes())
                .pipe(|b| {
                    String::from_utf8_lossy(b)
                        .to_string()
                        .pipe(MaybeWindowsPath)
                })
                .pipe(|path| (path, key.clone()))
        })
        .collect()
}

fn open_archive<'a>(path: &Path) -> Result<(ba2::fo4::Archive<'a>, ba2::fo4::ArchiveOptions)> {
    ba2::fo4::Archive::read(path)
        .context("opening archive")
        .with_context(|| format!("openinig archive at {path:#?}"))
}

#[derive(Debug, derive_more::From, derive_more::FromStr, derive_more::Display, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct MaybeWindowsPath(pub String);

impl MaybeWindowsPath {
    pub fn into_path(self) -> PathBuf {
        let s = self.0;
        let s = match s.contains("\\\\") {
            true => s.split("\\\\").collect::<Vec<_>>().join("/"),
            false => s,
        };
        let s = match s.contains("\\") {
            true => s.split("\\").collect::<Vec<_>>().join("/"),
            false => s,
        };
        PathBuf::from(s)
    }
}

pub(crate) fn create_file_all(path: &Path) -> Result<std::fs::File> {
    path.parent()
        .map(|parent| std::fs::create_dir_all(parent).with_context(|| format!("creating directory for [{}]", parent.display())))
        .unwrap_or_else(|| Ok(()))
        .and_then(|_| {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
                .with_context(|| format!("creating file [{}]", path.display()))
        })
        .with_context(|| format!("creating full path [{path:?}]"))
}

fn main() -> anyhow::Result<()> {
    Cli::parse().pipe(|Cli { command }| match command {
        ArchiveCommand::List { archive_path } => open_archive(&archive_path).map(|(archive, _)| {
            list_paths_with_originals(&archive)
                .into_iter()
                .enumerate()
                .for_each(|(idx, (file, key))| println!("{}. {}  ({:?})", idx + 1, file, key))
        }),
        ArchiveCommand::Extract { archive_path, file_path } => open_archive(&archive_path).and_then(|(archive, options)| {
            list_paths_with_originals(&archive).pipe(|entries| {
                entries
                    .iter()
                    .find(|(name, _key)| file_path.eq(name))
                    .with_context(|| format!("no [{file_path}] in {entries:#?}"))
                    .and_then(|(path, key)| {
                        archive
                            .get(key)
                            .context("opening using key")
                            .and_then(|archive_file| {
                                create_file_all(&path.clone().into_path())
                                    .context("creating output file")
                                    .and_then(|mut output_file| {
                                        archive_file
                                            .write(
                                                &mut output_file,
                                                &FileWriteOptions::builder()
                                                    .compression_format(options.compression_format())
                                                    .build(),
                                            )
                                            .context("writing to file")
                                    })
                            })
                    })
            })
        }),
    })
}
