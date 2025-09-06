use {
    crate::compression::case_insensitive_lookup::case_insensitive_string::CaseInsensitiveString,
    anyhow::{Context, Result},
    itertools::Itertools,
    std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    },
    tap::prelude::*,
};

pub mod case_insensitive_string {
    use {
        std::path::{Path, PathBuf},
        tap::Pipe,
    };

    #[derive(Debug, Clone)]
    pub struct CaseInsensitiveString {
        pub original: Box<str>,
        pub lowercase: Box<str>,
    }

    impl AsRef<str> for CaseInsensitiveString {
        fn as_ref(&self) -> &str {
            self.original.as_ref()
        }
    }

    impl CaseInsensitiveString {
        pub fn from_path(path: &Path) -> Self {
            path.to_string_lossy().pipe_deref(Self::new)
        }

        pub fn as_path(&self) -> PathBuf {
            self.original.as_ref().pipe(PathBuf::from)
        }
    }

    impl std::fmt::Display for CaseInsensitiveString {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "<case-insensitive>{}</case-insensitive>", self.original)
        }
    }

    impl PartialOrd for CaseInsensitiveString {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for CaseInsensitiveString {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.lowercase.cmp(&other.lowercase)
        }
    }

    impl CaseInsensitiveString {
        pub fn new(original: &str) -> Self {
            Self {
                lowercase: original.to_lowercase().pipe(Box::from),
                original: original.pipe(Box::from),
            }
        }
    }

    impl From<String> for CaseInsensitiveString {
        fn from(value: String) -> Self {
            Self {
                lowercase: value.to_lowercase().pipe(Box::from),
                original: value.pipe(Box::from),
            }
        }
    }
    impl From<&str> for CaseInsensitiveString {
        fn from(value: &str) -> Self {
            Self {
                lowercase: value.to_lowercase().pipe(Box::from),
                original: value.pipe(Box::from),
            }
        }
    }

    impl PartialEq for CaseInsensitiveString {
        fn eq(&self, other: &Self) -> bool {
            self.lowercase == other.lowercase
        }
    }

    impl Eq for CaseInsensitiveString {}
}

pub struct CaseInsensitiveArchiveListing<V>(BTreeMap<CaseInsensitiveString, V>);

pub type CaseInsenitiveBasicListing = CaseInsensitiveArchiveListing<()>;

#[derive(Debug, Clone)]
pub struct Entry<V> {
    pub archive_path: CaseInsensitiveString,
    pub requested_path: CaseInsensitiveString,
    pub extra_value: V,
}

#[allow(dead_code)]
impl CaseInsenitiveBasicListing {
    pub fn from_paths(entries: impl Iterator<Item = PathBuf>) -> Self {
        entries.map(|e| (e, ())).pipe(Self::from_paths_extra)
    }
    pub fn from_string_paths(entries: impl Iterator<Item = String>) -> Self {
        entries.map(|e| (e, ())).pipe(Self::from_string_paths_extra)
    }
    pub fn remove(&mut self, expected: CaseInsensitiveString) -> Result<CaseInsensitiveString> {
        self.remove_entry(expected).map(
            |Entry {
                 archive_path,
                 requested_path: _,
                 extra_value: (),
             }| archive_path,
        )
    }
}

impl<V> CaseInsensitiveArchiveListing<V> {
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn keys(&self) -> std::collections::btree_map::Keys<'_, CaseInsensitiveString, V> {
        self.0.keys()
    }

    pub fn from_paths_extra(entries: impl Iterator<Item = (PathBuf, V)>) -> Self {
        entries
            .map(|(path, value)| {
                (
                    path.to_string_lossy()
                        .pipe_deref(CaseInsensitiveString::from),
                    value,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .pipe(Self)
    }
    pub fn from_string_paths_extra(entries: impl Iterator<Item = (String, V)>) -> Self {
        entries
            .map(|(path, value)| (path.pipe_deref(CaseInsensitiveString::from), value))
            .collect::<BTreeMap<_, _>>()
            .pipe(Self)
    }

    pub fn plan_extract_list(mut self, expected: &[&Path]) -> Result<Vec<Entry<V>>> {
        expected
            .iter()
            .map(|path| {
                self.remove_entry(
                    path.to_string_lossy()
                        .pipe_deref(CaseInsensitiveString::new),
                )
            })
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("planning extraction of [{}] paths", expected.len()))
    }

    pub fn plan_extract_lookup(self, expected: &[&Path]) -> Result<BTreeMap<CaseInsensitiveString, Entry<V>>> {
        self.plan_extract_list(expected).map(|list| {
            list.into_iter()
                .map(|e| (e.archive_path.clone(), e))
                .collect()
        })
    }

    pub fn remove_entry(&mut self, expected: CaseInsensitiveString) -> Result<Entry<V>> {
        let Self(listed) = self;
        Err(())
            .or_else(|()| {
                listed
                    .remove_entry(&expected)
                    .with_context(|| format!("literal path '{expected:?}' not found"))
                    .map(|(archive_path, value)| Entry {
                        archive_path,
                        extra_value: value,
                        requested_path: expected.clone(),
                    })
            })
            .with_context(|| {
                const MAX_DEBUG_LEN: usize = 64;

                listed.keys().collect_vec().pipe(|keys| {
                    match keys.len() < MAX_DEBUG_LEN {
                        true => format!("{keys:#?}"),
                        false => keys
                            .iter()
                            .take(MAX_DEBUG_LEN)
                            .copied()
                            .collect_vec()
                            .pipe(|keys| format!("{keys:?}\n...and {} more", keys.len() - MAX_DEBUG_LEN)),
                    }
                    .pipe(|keys_debug| format!("when looking up a key [{expected:?}] inside:\n[{keys_debug}]"))
                })
            })
    }
}

impl<V> IntoIterator for CaseInsensitiveArchiveListing<V> {
    type IntoIter = std::collections::btree_map::IntoIter<CaseInsensitiveString, V>;
    type Item = <Self::IntoIter as IntoIterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_log::test]
    fn test_weird_key_1() -> Result<()> {
        let expected: &Path = Path::new("Meshes/armor/DryWells/Diegoarmor/diego_armorjumpsuit.nif");
        ["meshes/armor/DryWells/Diegoarmor/diego_armorjumpsuit.nif"]
            .into_iter()
            .map(PathBuf::from)
            .pipe(CaseInsensitiveArchiveListing::from_paths)
            .pipe(|listing| listing.plan_extract_list(&[expected]))
            .and_then(|lookup| {
                lookup
                    .iter()
                    .find(|e| {
                        e.requested_path.eq(&expected
                            .to_string_lossy()
                            .to_string()
                            .as_str()
                            .conv::<CaseInsensitiveString>())
                    })
                    .with_context(|| format!("expected '{expected:?}'"))
                    .map(|_| ())
            })
    }
}
