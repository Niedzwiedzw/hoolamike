use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct KindGuard<const VALUE: u8>;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WithKindGuard<const VALUE: u8, T> {
    #[serde(flatten)]
    pub inner: T,
    #[serde(rename = "Type")]
    pub kind: KindGuard<VALUE>,
}

mod serde_impl {
    use {
        super::KindGuard,
        serde::{Deserialize, Serialize},
        tap::TapFallible,
    };

    impl<const VALUE: u8> Serialize for KindGuard<VALUE> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::ser::Serializer,
        {
            serializer.serialize_u8(VALUE)
        }
    }

    impl<'de, const VALUE: u8> Deserialize<'de> for KindGuard<VALUE> {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::de::Deserializer<'de>,
        {
            let value: u8 = Deserialize::deserialize(deserializer)?;
            if value == VALUE {
                Ok(KindGuard)
            } else {
                Err(serde::de::Error::custom(format!("Expected \"{VALUE}\", but found \"{value}\""))).tap_err(|message| tracing::trace!(?message))
            }
        }
    }

    impl<const VALUE: u8> std::fmt::Display for KindGuard<VALUE> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            VALUE.fmt(f)
        }
    }
}
