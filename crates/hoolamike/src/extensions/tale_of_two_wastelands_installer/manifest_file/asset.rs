use {
    crate::utils::MaybeWindowsPath,
    anyhow::{Context, Result},
    serde::{Deserialize, Serialize},
    std::collections::BTreeMap,
    tap::prelude::*,
};

#[derive(Debug, serde_repr::Serialize_repr, serde_repr::Deserialize_repr, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub(crate) enum AssetRawKind {
    Copy = 0,
    New = 1,
    Patch = 2,
    XwmaFuz = 3,
    OggEnc2 = 4,
    AudioEnc = 5,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Params(String);

impl Params {
    pub fn parse(&self) -> anyhow::Result<BTreeMap<&str, &str>> {
        self.0
            .split_whitespace()
            .map(|param| {
                param
                    .split_once(":")
                    .context("param did not contain ':'")
                    .and_then(|(key, value)| {
                        key.split_once('-')
                            .with_context(|| format!("key [{key}] did not contain -"))
                            .map(|(_, key)| (key, value))
                    })
                    .with_context(|| format!("bad param: '{param}'"))
            })
            .collect::<Result<BTreeMap<_, _>>>()
            .with_context(|| format!("parsing params: '{}'", self.0))
    }
}

impl Params {
    pub const fn empty() -> Self {
        Self(String::new())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Status(u8);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tags(u16);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocationIndex(pub u8);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
pub struct FileName(pub MaybeWindowsPath);

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
enum AssetRaw {
    A(Tags, AssetRawKind, Params, Status, LocationIndex, LocationIndex, FileName),
    B(Tags, AssetRawKind, Params, Status, LocationIndex, LocationIndex, FileName, FileName),
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct FullLocation {
    pub location: LocationIndex,
    pub path: FileName,
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct MaybeFullLocation {
    pub location: LocationIndex,
    pub path: Option<FileName>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CopyAsset {
    pub tags: Tags,
    pub status: Status,
    pub source: FullLocation,
    pub target: MaybeFullLocation,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct NewAsset {
    pub tags: Tags,
    pub status: Status,
    pub source: FullLocation,
    pub target: MaybeFullLocation,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PatchAsset {
    pub tags: Tags,
    pub status: Status,
    pub source: FullLocation,
    pub target: MaybeFullLocation,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct XwmaFuzAsset {
    tags: u16,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct OggEnc2Asset {
    pub tags: Tags,
    pub status: Status,
    pub source: FullLocation,
    pub target: MaybeFullLocation,
    pub params: Params,
}
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct AudioEncAsset {
    pub tags: Tags,
    pub status: Status,
    pub source: FullLocation,
    pub params: Params,
    pub target: MaybeFullLocation,
}

#[derive(derive_more::From, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Asset {
    Copy(CopyAsset),
    New(NewAsset),
    Patch(PatchAsset),
    XwmaFuz(XwmaFuzAsset),
    OggEnc2(OggEnc2Asset),
    AudioEnc(AudioEncAsset),
}

impl Asset {
    pub fn target(&self) -> LocationIndex {
        match self {
            Asset::Copy(copy_asset) => copy_asset.target.location,
            Asset::New(new_asset) => new_asset.target.location,
            Asset::Patch(patch_asset) => patch_asset.target.location,
            Asset::OggEnc2(ogg_enc2_asset) => ogg_enc2_asset.target.location,
            Asset::AudioEnc(audio_enc_asset) => audio_enc_asset.target.location,
            Asset::XwmaFuz(_) => unimplemented!("Asset::XwmaFuz(_)"),
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Asset::Copy(copy_asset) => copy_asset.source.path.0 .0.as_str(),
            Asset::New(new_asset) => new_asset.source.path.0 .0.as_str(),
            Asset::Patch(patch_asset) => patch_asset.source.path.0 .0.as_str(),
            Asset::XwmaFuz(_) => "Asset::XwmaFuz IS NOT IMPLEMENTED",
            Asset::OggEnc2(ogg_enc2_asset) => ogg_enc2_asset.source.path.0 .0.as_str(),
            Asset::AudioEnc(audio_enc_asset) => audio_enc_asset.source.path.0 .0.as_str(),
        }
    }
}

impl From<&Asset> for AssetRawKind {
    fn from(value: &Asset) -> Self {
        match value {
            Asset::Copy(_) => Self::Copy,
            Asset::New(_) => Self::New,
            Asset::Patch(_) => Self::Patch,
            Asset::XwmaFuz(_) => Self::XwmaFuz,
            Asset::OggEnc2(_) => Self::OggEnc2,
            Asset::AudioEnc(_) => Self::AudioEnc,
        }
    }
}

impl From<Asset> for AssetRaw {
    fn from(val: Asset) -> Self {
        let kind = AssetRawKind::from(&val);
        match val {
            Asset::Copy(CopyAsset { tags, status, source, target }) => match target.path {
                Some(target_file_name) => AssetRaw::B(
                    tags,
                    kind,
                    Params::empty(),
                    status,
                    source.location,
                    target.location,
                    source.path,
                    target_file_name,
                ),
                None => AssetRaw::A(tags, kind, Params::empty(), status, source.location, target.location, source.path),
            },
            Asset::New(NewAsset { tags, status, source, target }) => match target.path {
                Some(target_file_name) => AssetRaw::B(
                    tags,
                    kind,
                    Params::empty(),
                    status,
                    source.location,
                    target.location,
                    source.path,
                    target_file_name,
                ),
                None => AssetRaw::A(tags, kind, Params::empty(), status, source.location, target.location, source.path),
            },
            Asset::Patch(PatchAsset { tags, status, source, target }) => match target.path {
                Some(target_file_name) => AssetRaw::B(
                    tags,
                    kind,
                    Params::empty(),
                    status,
                    source.location,
                    target.location,
                    source.path,
                    target_file_name,
                ),
                None => AssetRaw::A(tags, kind, Params::empty(), status, source.location, target.location, source.path),
            },
            Asset::OggEnc2(OggEnc2Asset {
                tags,
                status,
                source,
                target,
                params,
            }) => match target.path {
                Some(target_path) => AssetRaw::B(tags, kind, params, status, source.location, target.location, source.path, target_path),
                None => AssetRaw::A(tags, kind, params, status, source.location, target.location, source.path),
            },
            Asset::AudioEnc(AudioEncAsset {
                tags,
                status,
                source,
                target,
                params,
            }) => match target.path {
                Some(target_file_name) => AssetRaw::B(tags, kind, params, status, source.location, target.location, source.path, target_file_name),
                None => AssetRaw::A(tags, kind, params, status, source.location, target.location, source.path),
            },
            Asset::XwmaFuz(_xwma_fuz_asset) => unimplemented!("Asset::XwmaFuz"),
        }
    }
}

impl TryFrom<AssetRaw> for Asset {
    type Error = anyhow::Error;

    fn try_from(value: AssetRaw) -> Result<Self, Self::Error> {
        let tags;
        let operation;
        let params;
        let status;
        let location_location_index;
        let dest_location_location_index;
        let name;
        let dest_name;
        match value {
            AssetRaw::A(
                //
                f_flags,
                f_asset_raw_kind,
                f_params,
                f_status,
                f_location_index,
                f_location_index1,
                f_file_name,
            ) => {
                tags = f_flags;
                operation = f_asset_raw_kind;
                params = f_params;
                status = f_status;
                location_location_index = f_location_index;
                dest_location_location_index = f_location_index1;
                name = f_file_name;
                dest_name = None;
            }
            AssetRaw::B(
                //
                f_flags,
                f_asset_raw_kind,
                f_params,
                f_status,
                f_location_index,
                f_location_index1,
                f_file_name,
                f_dest_name,
            ) => {
                tags = f_flags;
                operation = f_asset_raw_kind;
                params = f_params;
                status = f_status;
                location_location_index = f_location_index;
                dest_location_location_index = f_location_index1;
                name = f_file_name;
                dest_name = Some(f_dest_name);
            }
        }

        match operation {
            AssetRawKind::Copy => CopyAsset {
                tags,
                status,
                source: FullLocation {
                    location: location_location_index,
                    path: name,
                },
                target: MaybeFullLocation {
                    location: dest_location_location_index,
                    path: dest_name,
                },
            }
            .pipe(Self::from),
            AssetRawKind::New => NewAsset {
                tags,
                status,
                source: FullLocation {
                    location: location_location_index,
                    path: name,
                },
                target: MaybeFullLocation {
                    location: dest_location_location_index,
                    path: dest_name,
                },
            }
            .pipe(Self::from),
            AssetRawKind::Patch => PatchAsset {
                tags,
                status,
                source: FullLocation {
                    location: location_location_index,
                    path: name,
                },
                target: MaybeFullLocation {
                    location: dest_location_location_index,
                    path: dest_name,
                },
            }
            .pipe(Self::from),
            AssetRawKind::OggEnc2 => OggEnc2Asset {
                tags,
                status,
                params,
                source: FullLocation {
                    location: location_location_index,
                    path: name,
                },
                target: MaybeFullLocation {
                    location: dest_location_location_index,
                    path: dest_name,
                },
            }
            .pipe(Self::from),
            AssetRawKind::AudioEnc => AudioEncAsset {
                tags,
                status,
                params,
                source: FullLocation {
                    location: location_location_index,
                    path: name,
                },
                target: MaybeFullLocation {
                    location: dest_location_location_index,
                    path: dest_name,
                },
            }
            .pipe(Self::from),
            AssetRawKind::XwmaFuz => anyhow::bail!("AssetRawKind::XwmaFuz"),
        }
        .pipe(anyhow::Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_log::test]
    fn test_deserialization_example_asset_1() -> anyhow::Result<()> {
        use anyhow::Context;
        serde_json::from_str::<Asset>(
            r#"[
  3073,
  0,
  "-f:24000 -q:5",
  5,
  9,
  25,
  "sound\\voice\\fallout3.esm\\maleuniquemisterburke\\ms11_ms11burkedisarmedchoi_00014f80_1.ogg",
  "sound\\voice\\fallout3.esm\\maleuniquemisterburke\\MS11_MS11BurkeDisarmedChoi_00014F80_1.ogg"
]"#,
        )
        .with_context(|| {
            format!(
                "{}\ncould not be parsed as {}",
                r#"[
  3073,
  0,
  "-f:24000 -q:5",
  5,
  9,
  25,
  "sound\\voice\\fallout3.esm\\maleuniquemisterburke\\ms11_ms11burkedisarmedchoi_00014f80_1.ogg",
  "sound\\voice\\fallout3.esm\\maleuniquemisterburke\\MS11_MS11BurkeDisarmedChoi_00014F80_1.ogg"
]"#,
                std::any::type_name::<self::Asset>()
            )
        })
        .and_then(|parsed| {
            serde_json::to_string_pretty(&parsed)
                .context("reserializing")
                .map(|asset| assert!(asset.contains("-f:24000"), "no -f:24000 in reserialized [{parsed:#?}]"))
                .with_context(|| format!("when reserializing [{parsed:#?}]"))
        })
    }
}

mod serde_compat;
