use {
    super::ProcessArchive,
    crate::{
        compression::ArchiveHandleKind,
        path::{ExistingPath, Path, PathBuf},
        progress_bars_v2::IndicatifWrapIoExt,
        utils::{ExistingPathRead, PathFileNameOrEmpty, ReadableCatchUnwindExt},
    },
    anyhow::{Context, Result},
    ba2::{BStr, ByteSlice, Reader},
    itertools::Itertools,
    std::{
        borrow::Cow,
        collections::{BTreeMap, BTreeSet},
        convert::identity,
        io::{Seek, Write},
        panic::catch_unwind,
        str::FromStr,
    },
    tap::prelude::*,
};

#[cfg(test)]
mod integration_tests;

type Fallout4Archive<'a> = (ba2::fo4::Archive<'a>, ba2::fo4::ArchiveOptions);
type Tes4Archive<'a> = (ba2::tes4::Archive<'a>, ba2::tes4::ArchiveOptions);

fn bethesda_path_to_path(bethesda_path: &[u8]) -> Result<PathBuf> {
    bethesda_path
        .to_str()
        .with_context(|| format!("converting [{}] to utf8", String::from_utf8_lossy(bethesda_path)))
        .map(ToOwned::to_owned)
        .and_then(|p| PathBuf::from_str(&p))
}

#[extension_traits::extension(pub trait Fallout4ArchiveCompat)]
impl Fallout4Archive<'_> {
    fn list_paths_with_originals(&self) -> Result<Vec<(PathBuf, ba2::fo4::ArchiveKey<'_>)>> {
        self.0
            .iter()
            .map(|(key, _file)| {
                key.name()
                    .to_str()
                    .context("name is not a valid string")
                    .map(|s| s.as_bytes())
                    .and_then(bethesda_path_to_path)
                    .map(|path| (path, key.to_owned()))
            })
            .collect::<Result<Vec<_>>>()
            .context("listing paths for bethesda archive")
    }
}

fn try_utf8(bstr: &BStr) -> Cow<'_, str> {
    bstr.to_str()
        .map(Cow::Borrowed)
        .context("file name is not a valid string")
        .unwrap_or_else(|error| {
            tracing::warn!(?error, non_utf_bytes=?bstr, "could not decode archive path as utf-8, using best-effort");
            bstr.to_str_lossy()
        })
}

#[extension_traits::extension(pub trait Tes4ArchiveCompat)]
impl Tes4Archive<'_> {
    fn list_paths_with_originals(&self) -> Result<Vec<(PathBuf, (ba2::tes4::ArchiveKey<'_>, ba2::tes4::DirectoryKey<'_>))>> {
        self.0
            .iter()
            .flat_map(|(archive_key, directory)| {
                directory
                    .iter()
                    .map(|(directory_key, _)| (archive_key.clone(), directory_key.clone()))
            })
            .map(|(archive_key, directory_key)| {
                (archive_key.name().pipe(try_utf8), directory_key.name().pipe(try_utf8))
                    .pipe(|(directory, filename)| PathBuf::from_str(directory.as_ref()).and_then(|directory| directory.join(filename)))
                    .map(|path| (path, (archive_key, directory_key)))
            })
            .collect::<Result<Vec<_>>>()
    }
}

impl super::ProcessArchive for Fallout4Archive<'_> {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_paths_with_originals()
            .map(|paths| paths.into_iter().map(|(p, _)| p).collect())
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        let options = ba2::fo4::FileWriteOptionsBuilder::new()
            .compression_format(self.1.compression_format())
            .build();

        self.list_paths_with_originals()
            .context("listing entries")
            .map(|paths| paths.into_iter().collect::<BTreeMap<_, _>>())
            .and_then(|mut source_paths| {
                paths
                    .iter()
                    .map(|path| {
                        source_paths
                            .remove(*path)
                            .with_context(|| format!("path [{path}] not found in [{:#?}]", source_paths.keys().collect_vec()))
                            .and_then(|repr| {
                                path.as_path()
                                    .named_tempfile_with_context()
                                    .context("creating temporary file for output")
                                    .map(|output| (repr, output))
                            })
                            .and_then(|(bethesda_path, output)| {
                                catch_unwind(|| self.0.get(&bethesda_path).context("could not read file"))
                                    .for_anyhow()
                                    .and_then(identity)
                                    .context("reading archive entry")
                                    .map(|file| (file, output))
                                    .and_then(|(file, mut output)| {
                                        catch_unwind(|| {
                                            file.iter()
                                                .map(|chunk| chunk.len() as u64)
                                                .sum::<u64>()
                                                .pipe(|size| {
                                                    let mut writer = tracing::Span::current().wrap_write(size, &mut output);
                                                    file.write(&mut writer, &options)
                                                        .context("writing fallout 4 bsa to output buffer")
                                                })
                                                .and_then(move |_| {
                                                    output.rewind().context("rewinding file").and_then(|_| {
                                                        tracing::debug!("finished dumping bethesda archive");
                                                        output.flush().context("flushing").map(|_| output)
                                                    })
                                                })
                                        })
                                        .for_anyhow()
                                        .and_then(identity)
                                        .context("extracting fallout 4 bsa")
                                    })
                            })
                            .context("extracting fallout 4 bsa")
                            .map(super::ArchiveFileHandle::Bethesda)
                            .map(|handle| ((*path).clone(), handle))
                            .with_context(|| format!("getting file handle for [{}] out of derived paths [{:#?}]", path, paths))
                            .context("extracting archive handle")
                    })
                    .collect::<Result<Vec<_>>>()
                    .context("not all archives could be extracted")
            })
            .with_context(|| format!("extracing [{}] paths from bethesda archive", paths.len()))
    }
}

impl super::ProcessArchive for Tes4Archive<'_> {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        self.list_paths_with_originals()
            .map(|paths| paths.into_iter().map(|(p, _)| p).collect::<Vec<_>>())
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        let options = {
            let version = self.1.version();
            move || ba2::tes4::FileCompressionOptions::builder().version(version)
        };
        paths
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .pipe_ref_mut(move |paths| {
                self.list_paths_with_originals()
                    .map(|paths_with_originals| {
                        paths_with_originals
                            .into_iter()
                            .filter(|(path, _)| {
                                paths.remove(path).tap(|exists_in_archive| {
                                    if !*exists_in_archive {
                                        tracing::trace!(?path, "ignoring path")
                                    }
                                })
                            })
                            .collect_vec()
                    })
                    .and_then(|filtered| {
                        filtered
                            .into_iter()
                            .map(|(path, (archive_key, directory_key))| {
                                self.0
                                    .get(&archive_key)
                                    .context("could not read directory")
                                    .and_then(|directory| {
                                        directory
                                            .get(&directory_key)
                                            .context("no file in directory")
                                    })
                                    .context("reading archive entry")
                                    .and_then(move |file| {
                                        path.as_path()
                                            .named_tempfile_with_context()
                                            .context("creating temporary file for output")
                                            .and_then(|mut output| {
                                                catch_unwind(|| {
                                                    file.len()
                                                        .pipe(|size| {
                                                            let mut writer = tracing::Span::current().wrap_write(size as _, &mut output);
                                                            // TODO: compression codec?
                                                            file.write(&mut writer, &options().build())
                                                                .context("writing fallout 4 bsa to output buffer")
                                                        })
                                                        .and_then(move |_| {
                                                            output.rewind().context("rewinding file").and_then(|_| {
                                                                tracing::debug!("finished dumping bethesda archive");
                                                                output.flush().context("flushing").map(|_| output)
                                                            })
                                                        })
                                                })
                                                .for_anyhow()
                                            })
                                            .and_then(identity)
                                            .context("extracting fallout 4 bsa")
                                            .map(super::ArchiveFileHandle::Bethesda)
                                            .with_context(|| format!("getting file handle for [{path}] out of derived paths"))
                                            .map(|handle| (path, handle))
                                    })
                            })
                            .collect::<Result<Vec<_>>>()
                    })
                    .and_then(|extracted| {
                        paths
                            .is_empty()
                            .then_some(extracted)
                            .with_context(|| format!("not all paths have been extracted, remaining:\n [{paths:#?}]"))
                    })
            })
    }
}

#[derive(Debug)]
pub enum BethesdaArchive<'a> {
    Fallout4(Fallout4Archive<'a>),
    Tes4(Tes4Archive<'a>),
}

impl ProcessArchive for BethesdaArchive<'_> {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        match self {
            BethesdaArchive::Fallout4(fo4) => fo4.list_paths(),
            BethesdaArchive::Tes4(tes4) => tes4.list_paths(),
        }
    }

    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, super::ArchiveFileHandle)>> {
        match self {
            BethesdaArchive::Fallout4(fo4) => fo4
                .get_many_handles(paths)
                .context("when reading from fallout 4 archive"),
            BethesdaArchive::Tes4(tes4) => tes4
                .get_many_handles(paths)
                .context("when reading from tes4 archive"),
        }
        .with_context(|| {
            format!(
                "when getting multiple handles out of an archive of kind [{kind:?}]",
                kind = ArchiveHandleKind::Bethesda
            )
        })
    }
}

impl BethesdaArchive<'_> {
    #[tracing::instrument]
    pub fn open(file: &ExistingPath) -> Result<Self> {
        file.open_file_read()
            .context("opening bethesda archive")
            .and_then(|(_path, mut archive)| {
                ba2::guess_format(&mut archive)
                    .context("unrecognized format")
                    .and_then(|format| {
                        (match format {
                            ba2::FileFormat::FO4 => ba2::fo4::Archive::read(file.as_os_path())
                                .context("opening fo4")
                                .map(BethesdaArchive::Fallout4),
                            ba2::FileFormat::TES3 => anyhow::bail!("{format:?} is not supported"),
                            ba2::FileFormat::TES4 => ba2::tes4::Archive::read(file.as_os_path())
                                .context("opening fo4")
                                .map(BethesdaArchive::Tes4),
                        })
                        .with_context(|| format!("opening archive based on guessed format: {format:?}"))
                    })
            })
    }
}

pub type BethesdaArchiveFile = tempfile::NamedTempFile;
