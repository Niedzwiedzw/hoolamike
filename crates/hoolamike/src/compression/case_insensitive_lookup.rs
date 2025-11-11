use {
    crate::path::CaseInsensitivePathBuf,
    anyhow::{Context, Result},
    itertools::Itertools,
    std::{collections::BTreeMap, path::Path, str::FromStr},
    tap::prelude::*,
};

pub struct CaseInsensitiveArchiveListing<V>(BTreeMap<CaseInsensitivePathBuf, V>);

pub type CaseInsenitiveBasicListing = CaseInsensitiveArchiveListing<()>;

#[allow(dead_code)]
impl CaseInsenitiveBasicListing {
    pub fn from_paths(entries: impl Iterator<Item = impl AsRef<Path>>) -> Result<Self> {
        entries.map(|e| (e, ())).pipe(Self::from_paths_extra)
    }
    pub fn from_string_paths(entries: impl Iterator<Item = impl AsRef<str>>) -> Result<Self> {
        entries.map(|e| (e, ())).pipe(Self::from_string_paths_extra)
    }
    pub fn remove(&mut self, expected: &CaseInsensitivePathBuf) -> Result<CaseInsensitivePathBuf> {
        self.remove_entry(expected).map(|_| expected.clone())
    }
}

impl<V> CaseInsensitiveArchiveListing<V> {
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn from_paths_extra(entries: impl Iterator<Item = (impl AsRef<Path>, V)>) -> Result<Self> {
        entries
            .map(|(path, value)| {
                path.as_ref()
                    .pipe(CaseInsensitivePathBuf::from_path)
                    .map(|path| (path, value))
            })
            .collect::<Result<BTreeMap<_, _>>>()
            .map(Self)
    }
    pub fn from_string_paths_extra(entries: impl Iterator<Item = (impl AsRef<str>, V)>) -> Result<Self> {
        entries
            .map(|(path, value)| {
                path.as_ref()
                    .pipe(CaseInsensitivePathBuf::from_str)
                    .map(|path| (path, value))
            })
            .collect::<Result<BTreeMap<_, _>>>()
            .map(Self)
    }
    /// keys are actual strings from lookup
    pub fn plan_extract_list(mut self, expected: &[&CaseInsensitivePathBuf]) -> Result<Vec<(CaseInsensitivePathBuf, V)>> {
        expected
            .iter()
            .map(|path| self.remove_entry(path))
            .collect::<Result<Vec<_>>>()
            .with_context(|| format!("planning extraction of [{}] paths", expected.len()))
    }

    pub fn plan_extract_lookup(self, expected: &[&CaseInsensitivePathBuf]) -> Result<BTreeMap<CaseInsensitivePathBuf, V>> {
        self.plan_extract_list(expected)
            .map(|list| list.into_iter().collect())
    }

    pub fn remove_entry(&mut self, expected: &CaseInsensitivePathBuf) -> Result<(CaseInsensitivePathBuf, V)> {
        self.0.remove_entry(expected).with_context(|| {
            const MAX_DEBUG_LEN: usize = 64;
            self.0.keys().collect_vec().pipe(|keys| {
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
    type IntoIter = std::collections::btree_map::IntoIter<CaseInsensitivePathBuf, V>;
    type Item = <Self::IntoIter as IntoIterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use {super::*, std::path::PathBuf};

    #[test_log::test]
    fn test_weird_key_1() -> Result<()> {
        let expected = Path::new("Meshes/armor/DryWells/Diegoarmor/diego_armorjumpsuit.nif").pipe(CaseInsensitivePathBuf::from_path)?;

        ["meshes/armor/DryWells/Diegoarmor/diego_armorjumpsuit.nif"]
            .into_iter()
            .map(PathBuf::from)
            .pipe(CaseInsensitiveArchiveListing::from_paths)
            .and_then(|listing| listing.plan_extract_list(&[&expected]))
            .and_then(|lookup| {
                lookup
                    .iter()
                    .find(|(e, _)| e.eq(&expected))
                    .with_context(|| format!("expected '{expected:?}'"))
                    .map(|_| ())
            })
    }
}
