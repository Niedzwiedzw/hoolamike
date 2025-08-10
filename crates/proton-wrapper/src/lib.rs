use {
    anyhow::{Context, Result},
    std::{path::PathBuf, process::Command},
};

pub struct ProtonContext {
    pub proton_path: PathBuf,
    pub prefix_dir: PathBuf,
    pub steam_path: PathBuf,
}

pub trait CommandWrapInProtonExt {
    fn wrap_in_proton(self, context: ProtonContext) -> Result<std::process::Command>;
}

impl CommandWrapInProtonExt for Command {
    fn wrap_in_proton(self, context: ProtonContext) -> Result<std::process::Command> {
        context.wrap(self)
    }
}

impl ProtonContext {
    pub fn wrap(self, command: Command) -> Result<Command> {
        let Self {
            proton_path,
            prefix_dir,
            steam_path,
        } = self;
        if !prefix_dir.exists() {
            std::fs::create_dir_all(&prefix_dir).with_context(|| format!("creating prefix for [{}]", prefix_dir.display()))?;
        }
        let mut wrapped = std::process::Command::new(proton_path);

        wrapped
            .arg("run")
            .arg(command.get_program())
            .args(command.get_args())
            .envs(command.get_envs().filter_map(|(k, v)| v.map(|v| (k, v))))
            .env("STEAM_COMPAT_DATA_PATH", prefix_dir)
            .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_path);
        if let Some(current_dir) = command.get_current_dir() {
            wrapped.current_dir(current_dir);
        }
        Ok(wrapped)
    }
}
