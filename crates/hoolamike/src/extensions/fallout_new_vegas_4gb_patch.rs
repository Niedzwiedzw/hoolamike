use {
    crate::utils::ExistingPathRead,
    anyhow::{Context, Result},
    case_insensitive_path::PathExistsUtf8Ext,
    hex_literal::hex,
    std::{
        io::{Read, Seek, SeekFrom, Write},
        ops::Div,
        path::Path,
    },
    tracing::{info, instrument},
};
type Sha1Hash = [u8; 20];

static RELEASE_IDS: &[&str] = &["US", "Unknown (DE?)", "RU"];

static PATCHED_HASHES: &[Sha1Hash] = &[
    hex!("0021023e37b1af143305a61b7b29a1811cc7c5fb"),
    hex!("37cae4e713b6b182311f66e31668d5005d1b9f5b"),
    hex!("600cd576cde7746fb2cd152fdd24db97453ed135"),
    hex!("34b65096caef9374dd6aa39af855e43308b417f2"),
];
static UNPATCHED_HASHES: &[Sha1Hash] = &[
    hex!("d068f394521a67c6e74fe572f59bd1be71e855f3"),
    hex!("07affda66c89f09b0876a50c77759640bc416673"),
    hex!("3980940522f0264ed9af14aea1773bb19f5160ab"),
    hex!("f65049b0957d83e61ecccacc730015ae77fb4c8b"),
    hex!("5394b94a18ffa6fa846e1d6033ad7f81919f13ac"),
    hex!("aca83d5a12a64af8854e381752fe989692d46e04"),
    hex!("946d2eaba04a75ff361b8617c7632b49f1ede9d3"),
];

static NVVERPATCH1: &[&[u8]] = &[b"\x3E\xF9\xFC".as_slice(), b"\x88\xC4\xFC".as_slice(), b"\x15\x43\xFD".as_slice()];
static NVVERPATCH2: &[&[u8]] = &[b"\x32\x32\x33\x38".as_slice(), b"\x32\x32\x33\x38".as_slice(), b"\x32\x32\x34\x39".as_slice()];

fn sha1_hash_file(file: &Path) -> Result<Sha1Hash> {
    use sha1::Digest;
    file.exists_utf8()
        .and_then(|file| {
            file.open_file_read()
                .map(|(_, file)| std::io::BufReader::new(file))
                .and_then(|mut file| {
                    let mut buf = vec![0u8; 8196];
                    let mut hasher = sha1::Sha1::new();
                    loop {
                        match file.read(&mut buf).context("reading chunk into a hasher")? {
                            0 => break,
                            size => hasher.update(&buf[..size]),
                        }
                    }
                    Ok(hasher.finalize())
                })
        })
        .map(|h| h.into())
}

#[instrument]
#[allow(clippy::needless_borrows_for_generic_args)]
#[rustfmt::skip]
pub fn patch_fallout_new_vegas(at_path: &Path) -> Result<()> {
    info!("checking");
    let current_hash = sha1_hash_file(at_path).context("checking current hash")?;

    info!(current_hash=%hex::encode(&current_hash), "hash caculated");

    if PATCHED_HASHES.contains(&current_hash) {
        info!("[SUCCESS] No need to patch, as the binary is already patched.");
        return Ok(());
    }
    let progv = UNPATCHED_HASHES
        .iter()
        .enumerate()
        .find_map(|(idx, hash)| hash.eq(&current_hash).then_some(idx.div(2)))
        .with_context(|| format!("unrecognized exe version: '{}'", hex::encode(&current_hash)))?;

    match progv {
        3 => {
            info!("patching FalloutNV.exe [GOG]...");
            self::apply_patch(at_path, &[
                (
                    0x00000148,
                    b"\x90\xE5\xBD",
                ),
                (
                    0x00000178,
                    b"\xD0\x4B\xF6",
                ),
                (
                    0x00BDD990,
                    b"\x68\xA0\xE5\xFD\x00\xFF\x15\xB0\xF0\xFD\x00\xE9\x3B\xDF\xEE\xFF\x6E\x76\x73\x65\x5F\x73\x74\x65\x61\x6D\x5F\x6C\x6F\x61\x64\x65\x72\x2E\x64\x6C\x6C",
                ),
            ])
        }
        0..=2 => {
            info!("patching FalloutNV.exe [{}]", RELEASE_IDS[progv]);
            let (op1loc, op2loc) = (NVVERPATCH1[progv], NVVERPATCH2[progv]);
            apply_patch(
                at_path,
                &[
                    (
                        0x00000136,
                        b"\x22",
                    ),
                    (
                        0x00000148,
                        b"\x20\xA6\x07",
                    ),
                    (
                        0x00000178,
                        op1loc,
                    ),
                    (
                        0x00F57277,
                        b"\xE8\x04\xFD\x06\x00\x90",
                    ),
                    (
                        0x00F57385,
                        b"\xE9\x56\xFC\x06\x00",
                    ),
                    (
                        0x00FC6F80,
                        b"\x90\x50\x50\x8B\xC4\x50\xB8\x40\x00\x00\x00\x50\xB8\x04\x00\x00\x00\x50\x8B\x85\x0C\xFC\xFF\xFF\x05\x0C\x02\x00\x00\x50\xFF\x55\x9C\x8B\x85\x0C\xFC\xFF\xFF\x05\x0C\x02\x00\x00\xC6\x00\x74\x8B\xC4\x50\x8B\x44\x24\x04\x50\xB8\x04\x00\x00\x00\x50\x8B\x85\x0C\xFC\xFF\xFF\x05\x0C\x02\x00\x00\x50\xFF\x55\x9C\x58\x58\xFF\xA5\x0C\xFC\xFF\xFF\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x50\x68\x00\xA6\x47\x01\xFF\x15\xB0\xF0\xFD\x00\x58\x5D\x5F\x5E\x5A\x59\x5B\xFF\xE0\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x6E\x76\x73\x65\x5F\x73\x74\x65\x61\x6D\x5F\x6C\x6F\x61\x64\x65\x72\x2E\x64\x6C\x6C",
                    ),
                    (
                        0x00FC7020,
                        b"\x60\x68\xC8\xA6\x47\x01\x68\xE0\xA6\x47\x01\xFF\x15\xF4\xF1\xFD\x00\x68\xC8\xA6\x47\x01\x68\xD0\xA6\x47\x01\xFF\x15\xF4\xF1\xFD\x00\x61\xE9\xA7\xEC\xF8\xFF",
                    ),
                    (
                        0x00FC70C8,
                        op2loc,
                    ),
                    (
                        0x00FC70CC,
                        b"\x30\x00\x00\x00\x53\x74\x65\x61\x6D\x41\x70\x70\x49\x64\x00\x00\x00\x00\x00\x00\x53\x74\x65\x61\x6D\x47\x61\x6D\x65\x49\x64",
                    ),   
                ]
            )
        }
        _ => unreachable!(),
    }
}
fn apply_patch(at_path: &Path, patch_chunks: &[(u64, &'static [u8])]) -> anyhow::Result<()> {
    std::fs::copy(
        at_path,
        at_path.with_added_extension(format!(
            "hoolamike-before-patch-{}",
            chrono::Local::now()
                .to_rfc3339()
                .replace(|c: char| !c.is_alphanumeric(), "-")
        )),
    )
    .context("making a backup file")?;
    std::fs::OpenOptions::new()
        .truncate(false)
        .create(false)
        .create_new(false)
        .append(false)
        .write(true)
        .open(at_path)
        .context("opening file for patching")
        .and_then(|mut file| {
            patch_chunks.iter().try_for_each(|(pos, data)| {
                file.seek(SeekFrom::Start(*pos))
                    .context("seeking")
                    .and_then(|_seeked| file.write_all(data).context("writing patch chunk"))
            })
        })
        .with_context(|| format!("applying patch to [{at_path:?}]"))
}
