use {
    anyhow::Context,
    case_insensitive_path::CaseInsensitivePathBuf,
    itertools::Itertools,
    nonempty::NonEmpty,
    serde::{Deserialize, Serialize, ser::Error as _},
    std::{
        iter::{empty, once},
        str::FromStr,
    },
    tap::prelude::*,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchiveHashPath {
    pub source_hash: String,
    pub path: Vec<CaseInsensitivePathBuf>,
}

impl std::fmt::Debug for ArchiveHashPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.pipe(|Self { source_hash, path }| write!(f, "[{source_hash}] {}", path.iter().map(|p| p.as_original_path()).join(" -> ")))
    }
}

impl Serialize for ArchiveHashPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.pipe(|Self { source_hash: root_hash, path }| {
            empty()
                .chain(once(root_hash.clone().pipe(Ok)))
                .chain(
                    path.iter()
                        .map(|p| serde_json::to_string(p).map_err(S::Error::custom)),
                )
                .collect::<Result<Vec<_>, _>>()
                .and_then(|output| output.serialize(serializer))
        })
    }
}

impl<'de> Deserialize<'de> for ArchiveHashPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        NonEmpty::<String>::deserialize(deserializer).and_then(|NonEmpty { head, tail }| {
            tail.into_iter()
                .map(|p| CaseInsensitivePathBuf::from_str(&p))
                .collect::<anyhow::Result<Vec<_>>>()
                .context("parsing archive hash path")
                .map_err(serde::de::Error::custom)
                .map(|path| ArchiveHashPath { source_hash: head, path })
        })
    }
}
