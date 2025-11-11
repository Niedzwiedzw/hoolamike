use {
    anyhow::{Context, Result},
    tap::prelude::*,
    tracing::{info, instrument},
};

mod linux {
    use super::*;

    #[instrument]
    pub fn register_nxm_handler() -> Result<()> {
        let current_exe = std::env::current_exe()
            .context("cno current exe found")?
            .display()
            .to_string();
        let crate_name = clap::crate_name!();

        let desktop_path = directories::UserDirs::new()
            .context("could not determine current user's directories")
            .map(|directories| directories.home_dir().to_owned())
            .context("figuring out home dir location")
            .tap_ok(|home| info!(?home, "deduced home directory"))
            .map(|home| home.join(".local").join("share").join("applications"))
            .tap_ok(|desktop| info!(?desktop, "deduced desktop entry directory"))
            .context("figuring out desktop directory")?;

        let desktop_entry_path = desktop_path.join(format!("{crate_name}.desktop"));
        info!(?desktop_entry_path, "deduced desktop entry path");

        let desktop_entry = format!(
            r#"
[Desktop Entry]
Type=Application
Categories=Game;
Name={crate_name} NXM handler
Exec="{current_exe}" %u
Terminal=true
NoDisplay=true
MimeType=x-scheme-handler/nxm;
"#
        )
        .trim()
        .to_string();
        info!("adding desktop entry:\n{desktop_entry}");
        std::fs::write(&desktop_entry_path, desktop_entry).with_context(|| format!("writing desktop entry to {desktop_entry_path:?}"))?;
        info!("wrote to {desktop_entry_path:?}");
        info!("running `update-desktop-database`");
        std::process::Command::new("update-desktop-database")
            .arg(desktop_path)
            .output()
            .context("bad status")
            .and_then(|o| {
                o.status
                    .success()
                    .then_some(())
                    .ok_or(o.status)
                    .map_err(|e| anyhow::anyhow!("Bad status: {e}"))
            })
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use {
        super::*,
        winreg::{RegKey, enums::*},
    };

    #[instrument]
    pub fn register_nxm_handler() -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (nxm_key, _) = hkcu
            .create_subkey("Software\\Classes\\nxm")
            .context("creating subkey")?;

        nxm_key
            .set_value("", &"URL:NXM Protocol")
            .context("setting nxm protocol key")?;
        nxm_key
            .set_value("URL Protocol", &"")
            .context("setting url protocol key")?;

        let (shell_key, _) = nxm_key.create_subkey("shell").context("subkey shell")?;
        let (open_key, _) = shell_key.create_subkey("open").context("subkey open")?;
        let (command_key, _) = open_key
            .create_subkey("command")
            .context("command subkey")?;

        let exe_path = format!(r#""{}" "%1""#, std::env::current_exe().context("no current exe")?.display());
        command_key
            .set_value("", &exe_path)
            .context("setting final value")?;
        info!("windows registry updated - current exe now handles nxm links");
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    #[instrument]
    pub fn register_nxm_handler() -> Result<()> {
        todo!("setting up nxm handler is not implemented on this platform (macos)")
    }
}

#[cfg(target_os = "linux")]
pub use linux::register_nxm_handler;
#[cfg(target_os = "macos")]
pub use macos::register_nxm_handler;
#[cfg(target_os = "windows")]
pub use windows::register_nxm_handler;
