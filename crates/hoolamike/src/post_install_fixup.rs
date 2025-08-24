use {
    crate::config_file::HoolamikeConfig,
    anyhow::{Context, Result},
    common::set_resolution,
    std::path::{Path, PathBuf},
    tap::prelude::*,
    tracing::{info, instrument},
};

#[instrument]
fn post_install_fixup_linux() -> Result<()> {
    info!("applying linux fixes");
    Ok(())
}

macro_rules! target_os_only {
    ($target_os:literal, $task:expr) => {{
        #[cfg(target_os = $target_os)]
        {
            $task
        }
        #[cfg(not(target_os = $target_os))]
        {
            Ok(())
        }
    }};
}

// fn find_file(by: impl Fn(&Path) -> bool) -> Result<Option<PathBuf>> {

// }

pub mod diffing;

#[extension_traits::extension(pub trait LinesPreservePlatform)]
impl str {
    fn lines_preserve_platform(&self) -> (&str, impl Iterator<Item = &str> + '_) {
        let sep = if self.contains("\r\n") { "\r\n" } else { "\n" };
        (sep, self.lines())
    }
}

pub mod common {
    use {super::*, crate::utils::ResultZipExt};

    macro_rules! re {
        ($name:ident, $regex:literal) => {
            pub static $name: once_cell::sync::Lazy<regex::Regex> =
                once_cell::sync::Lazy::new(|| regex::Regex::new($regex).expect(concat!("bad regex ", $regex)));
        };
    }

    pub fn patch_file<F: FnOnce(&str) -> Result<String>>(path: &Path, patch: F) -> Result<()> {
        std::fs::read_to_string(path)
            .with_context(|| format!("reading [{path:?}]"))
            .and_then(|before| {
                before.pipe_deref(patch).tap_ok(|after| {
                    diffing::PrettyDiff::new(&before, after).pipe(|diff| {
                        if !diff.is_empty() {
                            info!("applied change:\n{diff}")
                        }
                    })
                })
            })
            .context("applying patch")
            .and_then(|contents| std::fs::write(path, contents).context("writing patched contents"))
            .with_context(|| format!("patching file at [{path:?}]"))
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Resolution {
        pub x: u16,
        pub y: u16,
    }

    impl std::str::FromStr for Resolution {
        type Err = anyhow::Error;

        fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
            s.to_lowercase()
                .split_once("x")
                .context("no 'x' in resolution")
                .and_then(|(x, y)| {
                    x.parse::<u16>()
                        .context("parsing x")
                        .zip(y.parse::<u16>().context("parsing y"))
                        .context("parsing resolution components")
                })
                .map(|(x, y)| Resolution { x, y })
        }
    }

    impl std::fmt::Display for Resolution {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let Self { x, y } = self;
            write!(f, "{x}x{y}")
        }
    }

    fn list_all_files(cwd: &Path) -> impl Iterator<Item = PathBuf> + 'static {
        walkdir::WalkDir::new(cwd)
            .follow_links(false)
            .into_iter()
            .filter_map(|entry| {
                entry
                    .context("Bad entry")
                    .tap_err(|err| tracing::warn!(?err, "could not read entry"))
                    .ok()
            })
            .filter_map(|e| {
                e.path()
                    .pipe(|path| path.is_file().then(|| path.to_owned()))
            })
    }

    pub(crate) mod set_resolution {
        use {
            super::*,
            itertools::Itertools,
            std::borrow::Cow,
            tracing::{debug, info_span},
        };

        re!(RESOLUTION, r"^(#?)Resolution=.*");
        re!(FULLSCREEN, r"^(#?)Fullscreen=.*");
        re!(COMMENTED_FULLSCREEN, r"^(#?)#Fullscreen=.*");
        re!(BORDERLESS, r"^(#?)Borderless=.*");
        re!(COMMENTED_BORDERLESS, r"^(#?)#Borderless=.*");

        pub fn update_resolution(root: &Path, resolution: Resolution) -> Result<()> {
            let all_files_with_name = |name: &str| {
                let name = name.to_string();
                list_all_files(root).filter(move |file| {
                    file.file_name()
                        .map({
                            cloned![name];
                            move |filename| filename.to_string_lossy().eq(&name)
                        })
                        .unwrap_or_default()
                })
            };
            Ok(())
                .and_then(|_| {
                    info_span!("SSEDisplayTweaks.ini").in_scope(|| {
                        all_files_with_name("SSEDisplayTweaks.ini").try_for_each(|file| {
                            patch_file(&file, |contents| {
                                contents
                                    .lines_preserve_platform()
                                    .pipe(|(sep, lines)| {
                                        lines
                                            .map(|line| {
                                                if RESOLUTION.is_match(line) {
                                                    format!("Resolution={resolution}")
                                                } else if FULLSCREEN.is_match(line) {
                                                    "Fullscreen=false".to_string()
                                                } else if COMMENTED_FULLSCREEN.is_match(line) {
                                                    "#Fullscreen=false".to_string()
                                                } else if BORDERLESS.is_match(line) {
                                                    "Borderless=true".to_string()
                                                } else if COMMENTED_BORDERLESS.is_match(line) {
                                                    "#Borderless=true".to_string()
                                                } else {
                                                    line.to_string()
                                                }
                                            })
                                            .join(sep)
                                    })
                                    .pipe(Ok)
                            })
                            .tap_ok(|_| debug!("patched resolution to [{resolution}] at [{file:#?}]"))
                        })
                    })
                })
                .and_then(|_| {
                    info_span!("skyrimprefs.ini").in_scope(|| {
                        all_files_with_name("skyrimprefs.ini").try_for_each(|file| {
                            patch_file(&file, |contents| {
                                contents
                                    .lines_preserve_platform()
                                    .pipe(|(sep, lines)| {
                                        lines
                                            .map(|line| {
                                                if line.starts_with("iSize W") {
                                                    format!("iSize W = {}", resolution.x).pipe(Cow::Owned)
                                                } else if line.starts_with("iSize H") {
                                                    format!("iSize H = {}", resolution.y).pipe(Cow::Owned)
                                                } else {
                                                    line.pipe(Cow::Borrowed)
                                                }
                                            })
                                            .join(sep)
                                    })
                                    .pipe(Ok)
                            })
                            .tap_ok(|_| debug!("patched resolution to [{resolution}] at [{file:#?}]"))
                        })
                    })
                })
                .and_then(|_| {
                    info_span!("Fallout4Prefs.ini").in_scope(|| {
                        all_files_with_name("Fallout4Prefs.ini").try_for_each(|file| {
                            patch_file(&file, |contents| {
                                contents
                                    .lines_preserve_platform()
                                    .pipe(|(sep, lines)| {
                                        lines
                                            .map(|line| {
                                                if line.starts_with("iSize W") {
                                                    format!("iSize W = {}", resolution.x).pipe(Cow::Owned)
                                                } else if line.starts_with("iSize H") {
                                                    format!("iSize H = {}", resolution.y).pipe(Cow::Owned)
                                                } else {
                                                    line.pipe(Cow::Borrowed)
                                                }
                                            })
                                            .join(sep)
                                    })
                                    .pipe(Ok)
                            })
                            .tap_ok(|_| debug!("patched resolution to [{resolution}] at [{file:#?}]"))
                        })
                    })
                })
        }
    }
}

#[instrument]
fn post_install_fixup_common(config: &HoolamikeConfig) -> Result<()> {
    info!("common");
    Ok(())
        //
        .and_then(|_| {
            config
                .fixup
                .as_ref()
                .map(|crate::config_file::FixupConfig { game_resolution }| {
                    set_resolution::update_resolution(&config.installation.installation_path, *game_resolution)
                })
                .unwrap_or(Ok(()))
        })
}

#[instrument]
pub(crate) fn run_post_install_fixup(config: &HoolamikeConfig) -> Result<()> {
    info!("running post install fixup");
    Ok(())
        // platform-specific fixes
        .and_then(|_| target_os_only!("linux", post_install_fixup_linux()))
        .and_then(|_| post_install_fixup_common(config))
}
