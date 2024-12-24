use {
    crate::{
        downloaders::{helpers::FutureAnyhowExt, WithArchiveDescriptor},
        modlist_json::ArchiveDescriptor,
        progress_bars_v2::io_progress_style,
    },
    anyhow::{Context, Result},
    futures::{FutureExt, TryFutureExt},
    std::{future::ready, hash::Hasher, os::unix::fs::MetadataExt, path::PathBuf, sync::Arc},
    tap::prelude::*,
    tokio::io::AsyncReadExt,
    tracing_indicatif::span_ext::IndicatifSpanExt,
};

#[derive(Debug, Clone)]
pub struct DownloadCache {
    pub root_directory: PathBuf,
}
impl DownloadCache {
    pub fn new(root_directory: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root_directory)
            .context("creating download directory")
            .map(|_| Self {
                root_directory: root_directory.clone(),
            })
            .with_context(|| format!("creating download cache handler at [{}]", root_directory.display()))
    }
}

async fn read_file_size(path: &PathBuf) -> Result<u64> {
    tokio::fs::metadata(&path)
        .map_with_context(|| format!("getting size of {}", path.display()))
        .map_ok(|metadata| metadata.len())
        .await
}

#[tracing::instrument]
async fn calculate_hash(path: PathBuf) -> Result<u64> {
    let size = tokio::fs::metadata(&path)
        .await
        .context("no such file")?
        .size();

    let file_name = path
        .file_name()
        .context("file must have a name")?
        .to_string_lossy()
        .to_string();
    tracing::Span::current().pipe(|pb| {
        pb.pb_set_style(&io_progress_style());
        pb.pb_set_length(size);
        pb.pb_set_message(&file_name);
    });

    let mut file = tokio::fs::File::open(&path)
        .map_with_context(|| format!("opening file [{}]", path.display()))
        .await?;
    let mut buffer = vec![0; crate::BUFFER_SIZE];
    let mut hasher = xxhash_rust::xxh64::Xxh64::new(0);
    loop {
        match file.read(&mut buffer).await? {
            0 => break,
            read => {
                hasher.update(&buffer[..read]);
                tracing::Span::current().pb_inc(read as u64);
            }
        }
    }
    Ok(hasher.finish())
}

fn to_base_64(input: &[u8]) -> String {
    use base64::prelude::*;
    BASE64_STANDARD.encode(input)
}
fn from_base_64(input: impl AsRef<[u8]>) -> Result<Vec<u8>> {
    use base64::prelude::*;
    BASE64_STANDARD
        .decode(input)
        .context("decoding input as u64")
}

pub fn to_base_64_from_u64(input: u64) -> String {
    u64::to_ne_bytes(input).pipe(|bytes| to_base_64(&bytes))
}

pub fn to_u64_from_base_64(input: String) -> Result<u64> {
    from_base_64(&input)
        .and_then(|input| {
            input
                .as_slice()
                .try_conv::<[u8; 8]>()
                .context("invalid size")
        })
        .map(u64::from_ne_bytes)
        .context(input)
        .context("decoding string as hashed bytes")
}

pub async fn validate_hash(path: PathBuf, expected_hash: String) -> Result<PathBuf> {
    calculate_hash(path.clone())
        .map_ok(to_base_64_from_u64)
        .and_then(|hash| {
            hash.eq(&expected_hash)
                .then_some(path.clone())
                .with_context(|| format!("hash mismatch, expected [{expected_hash}], found [{hash}]"))
                .pipe(ready)
        })
        .await
        .with_context(|| format!("validating hash for [{}]", path.display()))
}

pub async fn validate_file_size(path: PathBuf, expected_size: u64) -> Result<PathBuf> {
    read_file_size(&path).await.and_then(move |found_size| {
        found_size
            .eq(&expected_size)
            .then_some(path)
            .with_context(|| format!("size mismatch (expected [{expected_size} bytes], found [{found_size} bytes])"))
    })
}

impl DownloadCache {
    pub fn download_output_path(&self, file_name: String) -> PathBuf {
        self.root_directory.join(file_name)
    }
    pub async fn verify(self: Arc<Self>, descriptor: ArchiveDescriptor) -> Result<WithArchiveDescriptor<PathBuf>> {
        let ArchiveDescriptor { hash, meta: _, name, size } = descriptor.clone();
        self.download_output_path(name)
            .pipe(Ok)
            .pipe(ready)
            .and_then(|expected_path| async move {
                tokio::fs::try_exists(&expected_path)
                    .map_with_context(|| format!("checking if path [{}] exists", expected_path.display()))
                    .map_ok(|exists| exists.then_some(expected_path.clone()))
                    .await
            })
            .and_then(|exists| match exists {
                Some(existing_path) => validate_file_size(existing_path.clone(), size)
                    .and_then(|found_path| validate_hash(found_path, hash))
                    .map_ok(Some)
                    .boxed(),
                None => None.pipe(Ok).pipe(ready).boxed(),
            })
            .await
            .and_then(|validated_path| {
                validated_path
                    .context("does not exist")
                    .map(|inner| WithArchiveDescriptor {
                        inner,
                        descriptor: descriptor.clone(),
                    })
            })
    }
}
