use {
    crate::{
        install_modlist::directives::nested_archive_manager::{max_open_files, WithPermit, OPEN_FILE_PERMITS},
        progress_bars_v2::IndicatifWrapIoExt,
        utils::PathReadWrite,
    },
    anyhow::{Context, Result},
    std::{
        ffi::OsStr,
        io::{Seek, Write},
        path::{Path, PathBuf},
        sync::Arc,
    },
    tap::prelude::*,
    tokio::sync::OwnedSemaphorePermit,
    tracing::{info_span, instrument, warn, Instrument},
    wrapped_7zip::WRAPPED_7ZIP,
};

pub mod preheated_archive;

pub mod bethesda_archive;
pub mod compress_tools;
pub mod sevenz;
pub mod unrar_rs;
pub mod zip;

#[cfg(test)]
pub mod detect_lzma_method_14;

pub mod forward_only_seek;

pub trait ProcessArchive: Sized {
    fn list_paths(&mut self) -> Result<Vec<PathBuf>>;
    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, self::ArchiveFileHandle)>>;
    #[tracing::instrument(skip_all)]
    fn get_handle(&mut self, path: &Path) -> Result<self::ArchiveFileHandle> {
        self.get_many_handles(&[path])
            .and_then(|out| out.into_iter().next().context("expected one output entry"))
            .map(|(_, handle)| handle)
    }
}

impl ProcessArchive for ArchiveHandle<'_> {
    #[instrument(skip(self), fields(kind=?ArchiveHandleKind::from(&*self)))]
    fn list_paths(&mut self) -> Result<Vec<PathBuf>> {
        match self {
            ArchiveHandle::Wrapped7Zip(i) => i.list_paths(),
            ArchiveHandle::Bethesda(i) => i.list_paths(),
            ArchiveHandle::CompressTools(i) => i.list_paths(),
            ArchiveHandle::Unrar(i) => i.list_paths(),
            ArchiveHandle::Zip(i) => i.list_paths(),
            ArchiveHandle::SevenzRust2(seven_zreader) => seven_zreader.list_paths(),
        }
        .with_context(|| format!("when listing paths of an archive of kind [{kind:?}]", kind = ArchiveHandleKind::from(&*self)))
    }

    #[instrument(skip(self), fields(kind=?ArchiveHandleKind::from(&*self)))]
    fn get_handle(&mut self, path: &Path) -> Result<self::ArchiveFileHandle> {
        match self {
            ArchiveHandle::Wrapped7Zip(i) => i.get_handle(path),
            ArchiveHandle::Bethesda(i) => i.get_handle(path),
            ArchiveHandle::CompressTools(i) => <_ as ProcessArchive>::get_handle(i, path),
            ArchiveHandle::Unrar(i) => i.get_handle(path),
            ArchiveHandle::Zip(i) => i.get_handle(path),
            ArchiveHandle::SevenzRust2(i) => i.get_handle(path),
        }
        .with_context(|| {
            format!(
                "when getting a file handle out of an archive of kind [{kind:?}]",
                kind = ArchiveHandleKind::from(&*self)
            )
        })
    }
    #[instrument(skip(self, paths), fields(kind=?ArchiveHandleKind::from(&*self), paths=%paths.len()))]
    fn get_many_handles(&mut self, paths: &[&Path]) -> Result<Vec<(PathBuf, self::ArchiveFileHandle)>> {
        match self {
            ArchiveHandle::Wrapped7Zip(i) => i.get_many_handles(paths),
            ArchiveHandle::Bethesda(i) => i.get_many_handles(paths),
            ArchiveHandle::CompressTools(i) => i.get_many_handles(paths),
            ArchiveHandle::Unrar(i) => i.get_many_handles(paths),
            ArchiveHandle::Zip(i) => i.get_many_handles(paths),
            ArchiveHandle::SevenzRust2(i) => i.get_many_handles(paths),
        }
        .with_context(|| {
            format!(
                "when getting multiple handles out of an archive of kind [{kind:?}]",
                kind = ArchiveHandleKind::from(&*self)
            )
        })
    }
}

#[allow(clippy::large_enum_variant)]
#[enum_dispatch::enum_dispatch]
pub enum ArchiveFileHandle {
    // CompressTools(compress_tools::CompressToolsFile),
    Wrapped7Zip((::wrapped_7zip::list_output::ListOutputEntry, ::wrapped_7zip::ArchiveFileHandle)),
    Bethesda(self::bethesda_archive::BethesdaArchiveFile),
    CompressTools(self::compress_tools::CompressToolsFile),
    Unrar(self::unrar_rs::UnrarFile),
    Zip(self::zip::ZipFile),
}

impl ArchiveFileHandle {
    #[tracing::instrument(skip(self))]
    pub fn size(&mut self) -> Result<u64> {
        match self {
            ArchiveFileHandle::Wrapped7Zip((entry, _)) => Ok(entry.size),
            ArchiveFileHandle::Bethesda(bethesda_archive_file) => bethesda_archive_file
                .stream_len()
                .context("could not seek to find the stream length"),
            ArchiveFileHandle::CompressTools(compress_tools_file) => compress_tools_file
                .stream_len()
                .context("could not seek to find stream length"),
            ArchiveFileHandle::Unrar(temp_path) => std::fs::metadata(temp_path)
                .context("reading metadata")
                .map(|m| m.len()),
            ArchiveFileHandle::Zip(temp_path) => std::fs::metadata(temp_path)
                .context("reading metadata")
                .map(|m| m.len()),
        }
    }
}

// static_assertions::assert_impl_all!(zip::ZipFile<'static>: Send, Sync);
// static_assertions::assert_impl_all!(compress_tools::CompressToolsFile: Send, Sync);
static_assertions::assert_impl_all!(::wrapped_7zip::ArchiveFileHandle: Send, Sync);
static_assertions::assert_impl_all!(self::bethesda_archive::BethesdaArchiveFile: Send, Sync);
static_assertions::assert_impl_all!(ArchiveFileHandle: Send, Sync);

impl ArchiveHandle<'_> {
    /// this is literally bruteforce approach
    pub fn with_guessed<T, F: FnMut(Self) -> Result<T> + Send + Sync>(path: &Path, extension: Option<&OsStr>, mut with_guessed: F) -> anyhow::Result<T> {
        match extension
            .map(|ext| ext.to_string_lossy())
            .map(|b| b.to_lowercase())
            .as_deref()
        {
            Some("bsa" | "ba2" | "mpi") => bethesda_archive::BethesdaArchive::open(path)
                .context("reading bsa")
                .map(Self::Bethesda)
                .and_then(&mut with_guessed)
                .tap_err(|message| tracing::warn!("could not open archive with Bethesda Archive Extractor: {message:?}")),
            Some("rar") => Err(())
                .or_else(|()| {
                    unrar_rs::ArchiveHandle::new(path)
                        .context("reading rar")
                        .map(Self::Unrar)
                        .and_then(&mut with_guessed)
                        .tap_err(|message| tracing::warn!("could not open archive with UnRar: {message:?}"))
                })
                .or_else(|reason| {
                    self::zip::ZipArchive::new(path)
                        .map(Self::Zip)
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with Zip: {message:?}"))
                })
                .or_else(|reason| {
                    path.open_file_read()
                        .and_then(|(_, file)| self::compress_tools::ArchiveHandle::new(file).map(Self::CompressTools))
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with CompressTools: {message:?}"))
                })
                .or_else(|reason| {
                    WRAPPED_7ZIP
                        .with(|wrapped| wrapped.open_file(path).map(Self::Wrapped7Zip))
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with 7z: {message:?}"))
                }),
            Some("7z") => Err(())
                .or_else(|reason| {
                    path.open_file_read()
                        .and_then(|(_, file)| {
                            self::sevenz::SevenZipArchive::new(file, "".into())
                                .context("opening archive with SevenzRust2 library")
                                .map(Box::new)
                                .map(Self::SevenzRust2)
                        })
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with SevenzRust2: {message:?}"))
                })
                .or_else(|reason| {
                    path.open_file_read()
                        .and_then(|(_, file)| self::compress_tools::ArchiveHandle::new(file).map(Self::CompressTools))
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with CompressTools: {message:?}"))
                })
                .or_else(|reason| {
                    WRAPPED_7ZIP
                        .with(|wrapped| wrapped.open_file(path).map(Self::Wrapped7Zip))
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with 7z: {message:?}"))
                }),
            Some("zip") => Err(())
                .or_else(|_| {
                    self::zip::ZipArchive::new(path)
                        .map(Self::Zip)
                        .and_then(&mut with_guessed)
                        .tap_err(|message| tracing::warn!("could not open archive with Zip: {message:?}"))
                })
                .or_else(|reason| {
                    path.open_file_read()
                        .and_then(|(_, file)| self::compress_tools::ArchiveHandle::new(file).map(Self::CompressTools))
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with CompressTools: {message:?}"))
                })
                .or_else(|reason| {
                    path.open_file_read()
                        .and_then(|(_, file)| {
                            self::sevenz::SevenZipArchive::new(file, "".into())
                                .context("opening archive with SevenzRust2 library")
                                .map(Box::new)
                                .map(Self::SevenzRust2)
                        })
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with SevenzRust2: {message:?}"))
                })
                .or_else(|reason| {
                    WRAPPED_7ZIP
                        .with(|wrapped| wrapped.open_file(path).map(Self::Wrapped7Zip))
                        .and_then(&mut with_guessed)
                        .with_context(|| format!("trying because: {reason:?}"))
                        .tap_err(|message| tracing::warn!("could not open archive with 7z: {message:?}"))
                }),
            other => {
                warn!("weird extension: [{other:?}] - it's guesswork at this point");
                Err(())
                    .or_else(|_| {
                        bethesda_archive::BethesdaArchive::open(path)
                            .context("reading bsa")
                            .map(Self::Bethesda)
                            .and_then(&mut with_guessed)
                            .tap_err(|message| tracing::warn!("could not open archive with Bethesda Archive Extractor: {message:?}"))
                    })
                    .or_else(|err| {
                        unrar_rs::ArchiveHandle::new(path)
                            .context("reading rar")
                            .map(Self::Unrar)
                            .and_then(&mut with_guessed)
                            .with_context(|| format!("because: {err:#?}"))
                            .tap_err(|message| tracing::warn!("could not open archive with Unrar: {message:?}"))
                    })
                    .or_else(|err| {
                        path.open_file_read()
                            .and_then(|(_, file)| self::compress_tools::ArchiveHandle::new(file).map(Self::CompressTools))
                            .and_then(&mut with_guessed)
                            .with_context(|| format!("because: {err:#?}"))
                            .tap_err(|message| tracing::warn!("could not open archive with CompressTools: {message:?}"))
                    })
                    .or_else(|reason| {
                        path.open_file_read()
                            .and_then(|(_, file)| {
                                self::sevenz::SevenZipArchive::new(file, "".into())
                                    .context("opening archive with SevenzRust2 library")
                                    .map(Box::new)
                                    .map(Self::SevenzRust2)
                            })
                            .and_then(&mut with_guessed)
                            .with_context(|| format!("trying because: {reason:?}"))
                            .tap_err(|message| tracing::warn!("could not open archive with SevenzRust2: {message:?}"))
                    })
                    .or_else(|err| {
                        WRAPPED_7ZIP
                            .with(|wrapped| wrapped.open_file(path).map(Self::Wrapped7Zip))
                            .and_then(&mut with_guessed)
                            .with_context(|| format!("because: {err:#?}"))
                            .tap_err(|message| tracing::warn!("could not open archive with 7z: {message:?}"))
                    })
                    .map_err(|error| anyhow::anyhow!("no defined archive handler could handle this file\n\n[{error:?}]"))
                    .with_context(|| format!("because no defined extension matched [{other:?}]"))
            }
        }
        .with_context(|| format!("no defined archive handler could handle this file: [{path:?}]"))
    }
}

impl std::io::Read for ArchiveFileHandle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            // ArchiveFileHandle::CompressTools(compress_tools_seek) => compress_tools_seek.read(buf),
            ArchiveFileHandle::Wrapped7Zip(wrapped_7zip) => wrapped_7zip.1.file.read(buf),
            ArchiveFileHandle::Bethesda(bethesda_archive_file) => bethesda_archive_file.read(buf),
            ArchiveFileHandle::CompressTools(compress_tools_file) => compress_tools_file.read(buf),
            ArchiveFileHandle::Unrar(temp_path) => temp_path.read(buf),
            ArchiveFileHandle::Zip(temp_path) => temp_path.read(buf),
        }
    }
}

#[derive(enum_kinds::EnumKind, derivative::Derivative)]
#[derivative(Debug)]
#[enum_kind(ArchiveHandleKind)]
pub enum ArchiveHandle<'a> {
    SevenzRust2(#[derivative(Debug = "ignore")] Box<sevenz::SevenZipArchive>),
    Wrapped7Zip(::wrapped_7zip::ArchiveHandle),
    Bethesda(bethesda_archive::BethesdaArchive<'a>),
    CompressTools(compress_tools::ArchiveHandle),
    Unrar(unrar_rs::ArchiveHandle),
    Zip(self::zip::ZipArchive),
}

pub mod wrapped_7zip;

#[extension_traits::extension(pub(crate) trait SeekWithTempFileExt)]
impl<T: std::io::Read + 'static> T
where
    Self: Sized,
{
    fn seek_with_temp_file_blocking_raw(mut self, expected_size: u64) -> Result<(u64, tempfile::TempPath)> {
        let _span = tracing::info_span!("seek_with_temp_file_blocking_raw").entered();
        tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
            .context("creating a tempfile")
            .and_then(|mut temp_file| {
                {
                    let writer = &mut info_span!("writing_file").wrap_write(expected_size, &mut temp_file);
                    std::io::copy(&mut self, writer)
                }
                .context("creating a seekable temp file")
                .map(|wrote_size| {
                    wrote_size
                        .eq(&expected_size)
                        .then_some(wrote_size)
                        .with_context(|| format!("error when writing temp file: expected [{expected_size}], found [{wrote_size}]"))
                        .tap_err(|bad_size| tracing::debug!(?bad_size))
                        .pipe(|_| wrote_size)
                })
                .map(|wrote_size| (wrote_size, temp_file))
                .and_then(|(wrote_size, mut file)| {
                    file.flush()
                        .context("flushing file")
                        .map(|_| file.into_temp_path())
                        .map(|path| (wrote_size, path))
                })
            })
    }

    fn seek_with_temp_file_blocking_unbounded(self, expected_size: u64, _computation_permit: OwnedSemaphorePermit) -> Result<(u64, tempfile::TempPath)> {
        let _span = tracing::info_span!("seek_with_temp_file_blocking_unbounded").entered();
        self.seek_with_temp_file_blocking_raw(expected_size)
    }
    fn seek_with_temp_file_blocking(mut self, expected_size: u64, permit: tokio::sync::OwnedSemaphorePermit) -> Result<WithPermit<tempfile::TempPath>> {
        let _span = tracing::info_span!("seek_with_temp_file_blocking").entered();
        tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
            .context("creating a tempfile")
            .and_then(|mut temp_file| {
                {
                    let writer = &mut info_span!("writing_file")
                        .clone()
                        .wrap_write(expected_size, &mut temp_file);
                    std::io::copy(&mut self, writer)
                }
                .context("creating a seekable temp file")
                .and_then(|wrote_size| {
                    wrote_size
                        .eq(&expected_size)
                        .then_some(wrote_size)
                        .with_context(|| format!("error when writing temp file: expected [{expected_size}], found [{wrote_size}]"))
                })
                .map(|_| temp_file)
                .and_then(|mut file| {
                    file.flush()
                        .context("flushing file")
                        .map(|_| file.into_temp_path())
                })
            })
            .map(|file| WithPermit { permit, inner: file })
    }
    async fn seek_with_temp_file(self, expected_size: u64) -> Result<WithPermit<tempfile::TempPath>>
    where
        T: Sync + Send + 'static,
    {
        let span = tracing::info_span!(
            "seek_with_temp_file",
            acquired_file_permits=%(max_open_files() - OPEN_FILE_PERMITS.available_permits()),
            max_open_files=%max_open_files(),
        );
        let reader = Arc::new(std::sync::Mutex::new(self));
        WithPermit::new_blocking(&OPEN_FILE_PERMITS, {
            cloned![span];
            move || {
                let span = span.entered();
                tempfile::NamedTempFile::new_in(*crate::consts::TEMP_FILE_DIR)
                    .context("creating a tempfile")
                    .and_then(|mut temp_file| {
                        {
                            let mut reader = reader.lock().unwrap();
                            let writer = &mut span.clone().wrap_write(expected_size, &mut temp_file);
                            std::io::copy(&mut *reader, writer)
                        }
                        .context("creating a seekable temp file")
                        .and_then(|wrote_size| {
                            wrote_size
                                .eq(&expected_size)
                                .then_some(wrote_size)
                                .with_context(|| format!("error when writing temp file: expected [{expected_size}], found [{wrote_size}]"))
                        })
                        .map(|_| temp_file)
                        .and_then(|mut file| {
                            file.flush()
                                .context("flushing file")
                                .map(|_| file.into_temp_path())
                        })
                    })
            }
        })
        .instrument(span)
        .await
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        futures::{StreamExt, TryFutureExt, TryStreamExt},
        std::io::BufReader,
    };
    #[test_log::test(tokio::test)]
    async fn test_seek_with_tempfile() -> Result<()> {
        [
            //
            [1u8; 8].as_slice(),
        ]
        .pipe(futures::stream::iter)
        .map(|slice| (slice, slice.pipe(std::io::Cursor::new).pipe(BufReader::new)))
        .map(Ok)
        .try_for_each(|(slice, reader)| {
            reader
                .seek_with_temp_file(slice.len() as _)
                .and_then(move |temp| async move {
                    let buffer = temp
                        .inner
                        .pipe(|f| std::fs::read(&f))
                        .context("reading failed")?;
                    assert_eq!(slice, &buffer, "buffer mismatch");
                    Ok(())
                })
        })
        .await
    }
}
