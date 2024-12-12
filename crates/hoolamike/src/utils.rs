use {
    itertools::Itertools,
    serde::{Deserialize, Serialize},
    std::path::PathBuf,
    tap::prelude::*,
};

#[extension_traits::extension(pub trait ReadableCatchUnwindExt)]
impl<T> std::result::Result<T, Box<dyn std::any::Any + Send>> {
    fn for_anyhow(self) -> anyhow::Result<T> {
        self.map_err(ReadableCatchUnwindErrorExt::to_readable_error)
    }
}

#[extension_traits::extension(pub trait ReadableCatchUnwindErrorExt)]
impl Box<dyn std::any::Any + Send> {
    fn to_readable_error(self) -> anyhow::Error {
        if let Some(message) = self.downcast_ref::<&str>() {
            format!("Caught panic with message: {}", message)
        } else if let Some(message) = self.downcast_ref::<String>() {
            format!("Caught panic with message: {}", message)
        } else {
            "Caught panic with an unknown type.".to_string()
        }
        .pipe(|e| anyhow::anyhow!("{e}"))
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, PartialOrd, Hash, derive_more::Display, Clone)]
pub struct MaybeWindowsPath(pub String);

impl MaybeWindowsPath {
    pub fn into_path(self) -> PathBuf {
        let s = self.0;
        let s = match s.contains("\\\\") {
            true => s.split("\\\\").join("/"),
            false => s,
        };
        let s = match s.contains("\\") {
            true => s.split("\\").join("/"),
            false => s,
        };
        PathBuf::from(s)
    }
}

pub fn boxed_iter<'a, T: 'a>(iter: impl Iterator<Item = T> + 'a) -> Box<dyn Iterator<Item = T> + 'a> {
    Box::new(iter)
}