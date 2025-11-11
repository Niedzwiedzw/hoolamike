use {
    crate::utils::{IteratorTryFindMap, ResultZipExt},
    anyhow::{Context, Result},
    std::{borrow::Borrow, ops::Deref, str::FromStr},
    tap::{Pipe, Tap},
    typed_path::{Utf8Path, Utf8PlatformEncoding, Utf8PlatformPath, Utf8PlatformPathBuf, Utf8TypedPath, Utf8TypedPathBuf, Utf8UnixPathBuf, Utf8WindowsPathBuf},
};

mod utils;

/// This emulates windows path behaviour, hopefully without being too annoying
#[derive(Debug, Clone)]
pub struct CaseInsensitivePathBuf {
    original: Utf8TypedPathBuf,
    /// used for [std::cmp::Ord] and [std::hash::Hash]
    lowercase: Utf8UnixPathBuf,
    native: Utf8PlatformPathBuf,
}

impl PartialEq for CaseInsensitivePathBuf {
    fn eq(&self, other: &Self) -> bool {
        self.lowercase == other.lowercase
    }
}

impl From<&ExistingPath> for CaseInsensitivePathBuf {
    fn from(val: &ExistingPath) -> Self {
        CaseInsensitivePathBuf::from_str(val.as_path().as_str()).expect("path exists so it MUST be representable on all platforms")
    }
}

impl From<ExistingPathBuf> for CaseInsensitivePathBuf {
    fn from(value: ExistingPathBuf) -> Self {
        value.into()
    }
}

impl Eq for CaseInsensitivePathBuf {}

impl std::cmp::PartialOrd for CaseInsensitivePathBuf {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.lowercase.partial_cmp(&other.lowercase)
    }
}

impl std::cmp::Ord for CaseInsensitivePathBuf {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

#[derive(Debug, derive_more::Display, Clone)]
pub struct ExistingPathBuf(Utf8PlatformPathBuf);

impl ExistingPathBuf {
    pub fn case_insensitive(&self) -> CaseInsensitivePathBuf {
        CaseInsensitivePathBuf::from(self.clone())
    }
    fn new_native_unchecked(path: Utf8PlatformPathBuf) -> Self {
        Self(path)
    }
    pub fn new_native(path: &Utf8PlatformPath) -> anyhow::Result<Self> {
        Self::new(std::path::Path::new(path))
    }

    pub fn new(path: &std::path::Path) -> anyhow::Result<Self> {
        path.exists()
            .then_some(path)
            .context("path doesn't exist")
            .and_then(|path| path.utf8_platform_path())
            .with_context(|| format!("validating presumably existing path: {path:?}"))
            .map(Self)
    }
}

impl AsRef<std::path::Path> for ExistingPathBuf {
    fn as_ref(&self) -> &std::path::Path {
        AsRef::<ExistingPath>::as_ref(self).as_ref()
    }
}

impl AsRef<std::path::Path> for ExistingPath {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }
}
impl ExistingPath {
    #[inline]
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Self {
        unsafe { &*(s.as_ref() as *const str as *const Self) }
    }

    pub fn as_path(&self) -> &Utf8PlatformPath {
        &self.0
    }

    pub fn case_insensitive(&self) -> CaseInsensitivePathBuf {
        CaseInsensitivePathBuf::from(self)
    }
}

#[derive(Debug, derive_more::Display)]
pub struct ExistingPath(Utf8PlatformPath);

impl ExistingPath {
    pub fn into_owned(&self) -> ExistingPathBuf {
        self.to_owned()
    }

    pub fn as_os_path(&self) -> &std::path::Path {
        std::path::Path::new(&self.0)
    }

    pub fn join_checked(&self, segment: &str) -> Result<ExistingPathBuf> {
        self.as_path()
            .join_checked(segment)
            .context("bad path")
            .and_then(|child| ExistingPathBuf::new_native(&child))
            .with_context(|| format!("joining [{self}] with {segment}"))
    }

    pub fn join_new(&self, segment: &str) -> Result<Utf8PlatformPathBuf> {
        self.0
            .join_checked(segment)
            .with_context(|| format!("joining [{segment}] to {self}"))
    }
}

impl Deref for ExistingPathBuf {
    type Target = ExistingPath;

    #[inline]
    fn deref(&self) -> &ExistingPath {
        ExistingPath::new(&self.0)
    }
}

impl Borrow<ExistingPath> for ExistingPathBuf {
    fn borrow(&self) -> &ExistingPath {
        self.deref()
    }
}

impl AsRef<ExistingPath> for ExistingPathBuf {
    fn as_ref(&self) -> &ExistingPath {
        self
    }
}

impl ToOwned for ExistingPath {
    type Owned = ExistingPathBuf;

    fn to_owned(&self) -> Self::Owned {
        ExistingPathBuf(self.0.to_owned())
    }
}

impl CaseInsensitivePathBuf {
    pub fn normalize(&self) -> Self {
        self.pipe(|Self { original, lowercase, native }| Self {
            original: original.normalize(),
            lowercase: lowercase.normalize(),
            native: native.normalize(),
        })
    }
    pub fn extension(&self) -> Option<&str> {
        self.original.extension()
    }
    /// same as [Self::exists] but errors if the path doesn't exist
    pub fn try_exists(&self) -> Result<ExistingPathBuf> {
        self.exists()
            .and_then(|exists| exists.with_context(|| format!("path {self} does not exist")))
    }
    pub fn exists(&self) -> Result<Option<ExistingPathBuf>> {
        self.native.pipe_ref(|native| {
            let mut components = native.iter().map(|c| c.to_lowercase());
            match native.is_absolute() {
                false => typed_path::utils::utf8_current_dir()
                    .context("reading current directory as utf8")
                    .map(Some),
                true => components
                    .next()
                    .map(|root| Utf8PlatformPathBuf::new().tap_mut(|p| p.push(root)))
                    .pipe(Ok),
            }
            .and_then(|root| {
                root.map(|root| {
                    components.try_fold(Some(root), |cwd, next_component| {
                        cwd.context("bad cwd").and_then(|cwd| {
                            std::fs::read_dir(std::path::Path::new(cwd.as_str()))
                                .context("reading current directory")
                                .and_then(|mut entries| {
                                    entries
                                        .try_find_map(|d| {
                                            d.context("bad entry").map(|e| {
                                                e.path()
                                                    .file_name()
                                                    .expect("must be normalized at this point")
                                                    .to_string_lossy()
                                                    .pipe_deref(|entry| {
                                                        entry.eq(&next_component).then(|| {
                                                            cwd.clone()
                                                                .tap_mut(|cwd| cwd.push_checked(entry).expect("checked path failed"))
                                                        })
                                                    })
                                            })
                                        })
                                        .context("finding matching path")
                                })
                                .transpose()
                                .transpose()
                        })
                    })
                })
                .transpose()
                .map(|o| o.flatten().map(ExistingPathBuf::new_native_unchecked))
                .context("crawling the directory tree to case-insensitively match on the path")
            })
        })
    }

    pub fn join_case_insensitive(&self, other: Self) -> Result<Self> {
        self.pipe(|Self { original, lowercase, native }| {
            original
                .join_checked(&other.original)
                .context("joining original")
                .zip(
                    lowercase
                        .join_checked(&other.lowercase)
                        .context("joining lowercase"),
                )
                .zip(native.join_checked(&other.native).context("joining native"))
                .with_context(|| format!("joining `{self:#?}` with {other:#?}"))
                .context("performing join of 2 case insensitive paths")
                .map(|((original, lowercase), native)| Self { original, lowercase, native })
        })
    }
    pub fn join(&self, segment: impl AsRef<str>) -> Result<Self> {
        let segment = segment.as_ref();
        Self::from_str(segment)
            .context("normalizing segment")
            .and_then(|segment| self.join_case_insensitive(segment))
            .with_context(|| format!("joining {self} + {segment}"))
    }

    pub fn as_original_path(&self) -> Utf8TypedPath<'_> {
        self.original.to_path()
    }

    pub fn as_original_std_path(&self) -> std::path::PathBuf {
        std::path::Path::new(self.as_original_path().as_str()).to_owned()
    }
}

pub type PathBuf = self::CaseInsensitivePathBuf;
pub type Path = self::CaseInsensitivePathBuf;

impl AsRef<self::Path> for CaseInsensitivePathBuf {
    fn as_ref(&self) -> &self::Path {
        self
    }
}

impl CaseInsensitivePathBuf {
    pub fn from_bytes(path: Vec<u8>) -> Result<Self> {
        String::from_utf8(path)
            .context("path is not utf8")
            .and_then(|path| path.parse())
    }
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        Self::from_bytes(path.as_os_str().as_encoded_bytes().to_vec())
    }
    pub fn as_path(&self) -> &Utf8Path<Utf8PlatformEncoding> {
        self.native.as_path()
    }
}

impl Utf8TypedPathToPlatformExt for Utf8TypedPathBuf {
    fn into_platform_encoding_checked(&self) -> Result<Utf8PlatformPathBuf> {
        self.to_path().into_platform_encoding_checked()
    }

    fn into_unix_encoding_checked(&self) -> Result<Utf8UnixPathBuf> {
        self.to_path().into_unix_encoding_checked()
    }

    fn into_windows_encoding_checked(&self) -> Result<Utf8WindowsPathBuf> {
        self.to_path().into_windows_encoding_checked()
    }
}

#[extension_traits::extension(pub trait Utf8TypedPathToPlatformExt)]
impl Utf8TypedPath<'_> {
    fn into_platform_encoding_checked(&self) -> Result<Utf8PlatformPathBuf> {
        match self {
            Utf8TypedPath::Unix(i) => i.with_platform_encoding_checked(),
            Utf8TypedPath::Windows(i) => i.with_platform_encoding_checked(),
        }
        .with_context(|| format!("converting [{self:?}] to platform encoding"))
    }

    fn into_unix_encoding_checked(&self) -> Result<Utf8UnixPathBuf> {
        self.with_unix_encoding_checked()
            .context("changing encoding")
            .and_then(|m| match m {
                Utf8TypedPathBuf::Unix(u) => Ok(u),
                Utf8TypedPathBuf::Windows(windows) => Err(anyhow::anyhow!("expected this to be unix: {windows}")),
            })
    }
    fn into_windows_encoding_checked(&self) -> Result<Utf8WindowsPathBuf> {
        self.with_windows_encoding_checked()
            .context("changing encoding")
            .and_then(|m| match m {
                Utf8TypedPathBuf::Windows(u) => Ok(u),
                Utf8TypedPathBuf::Unix(unix) => Err(anyhow::anyhow!("expected this to be windows: {unix}")),
            })
    }
}

impl std::hash::Hash for CaseInsensitivePathBuf {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.lowercase, state)
    }
}

impl FromStr for CaseInsensitivePathBuf {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let original = Utf8TypedPath::derive(s).normalize();
        Utf8TypedPathBuf::from(original.as_str().to_lowercase())
            .into_unix_encoding_checked()
            .context("normalizing to unix encoding for comparison")
            .zip(
                original
                    .into_platform_encoding_checked()
                    .context("converting original to native (platform) encoding"),
            )
            .map(|(lowercase, native)| Self { original, lowercase, native })
            .with_context(|| format!("interpreting `{s}` as case insensitive path"))

        // Utf8TypedPathBuf::from(original.as_str().to_lowercase())
        //            .into_unix_encoding_checked()
        //            .context("normalizing to unix encoding for comparison")
        //            .zip(
        //                original
        //            .into_platform_encoding_checked()
        //            .context("converting original to native (platform) encoding")

        //        ).map(|(lowercase, native)| {
        //                Self { original, native, lowercase }
        //            })
    }
}

impl std::fmt::Display for CaseInsensitivePathBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.original.as_str().fmt(f)
    }
}

#[extension_traits::extension(pub trait PathExistsUtf8Ext)]
impl<P: AsRef<std::path::Path>> P {
    fn exists_utf8(&self) -> anyhow::Result<ExistingPathBuf> {
        ExistingPathBuf::new(self.as_ref())
    }

    #[allow(async_fn_in_trait)]
    /// TODO: placeholder
    #[cfg(feature = "tokio")]
    async fn exists_utf8_async(&self) -> anyhow::Result<ExistingPathBuf> {
        ::tokio::task::block_in_place(|| self.exists_utf8())
    }
    fn utf8_platform_path(&self) -> anyhow::Result<Utf8PlatformPathBuf> {
        let path: &std::path::Path = self.as_ref();
        String::from_utf8(path.as_os_str().as_encoded_bytes().to_vec())
            .with_context(|| format!("not utf8: {path:?}"))
            .and_then(|v| Utf8TypedPath::derive(v.as_str()).into_platform_encoding_checked())
    }
}

#[extension_traits::extension(pub trait IntoUtf8CaseInsensitivePath)]
impl<P: AsRef<std::path::Path>> P {
    fn case_insensitive_utf8(&self) -> anyhow::Result<CaseInsensitivePathBuf> {
        CaseInsensitivePathBuf::from_path(self.as_ref())
    }
}

#[cfg(feature = "tokio")]
mod tokio {
    use super::*;
    impl ExistingPath {
        pub async fn open_file_read_async(&self) -> Result<(super::ExistingPathBuf, ::tokio::fs::File)> {
            ::tokio::fs::OpenOptions::new()
                .read(true)
                .open(self.as_path())
                .await
                .context("opening file")
                .map(|file| (self.to_owned(), file))
                .with_context(|| format!("opening file at [{self}]"))
        }
    }
    impl CaseInsensitivePathBuf {
        /// for now it's a placeholder, we'll see if it will be a problem
        pub async fn exists_async(&self) -> Result<Option<ExistingPathBuf>> {
            ::tokio::task::block_in_place(|| self.exists())
        }
        /// for now it's a placeholder, we'll see if it will be a problem
        pub async fn try_exists_async(&self) -> Result<ExistingPathBuf> {
            ::tokio::task::block_in_place(|| self.try_exists())
        }
    }
}

mod serde {
    use {
        super::CaseInsensitivePathBuf,
        serde::{Deserialize, Deserializer, Serialize, Serializer},
    };

    impl Serialize for CaseInsensitivePathBuf {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            self.original.as_str().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for CaseInsensitivePathBuf {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            String::deserialize(deserializer).and_then(|string| string.parse::<Self>().map_err(serde::de::Error::custom))
        }
    }
}
