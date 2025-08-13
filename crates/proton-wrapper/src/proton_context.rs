use {
    crate::ipc::{ProtonWrapperShellBin, SerializedCommand, WrappedStdout},
    anyhow::{anyhow, Context, Result},
    itertools::Itertools,
    std::{
        fs::File,
        io::Read,
        ops::Not,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        sync::Arc,
    },
    tap::{Pipe, Tap, TapFallible},
    tempfile::TempDir,
    tracing::{debug, info, instrument},
    typed_path::{Utf8UnixPath, Utf8WindowsPath, Utf8WindowsPathBuf},
};

static PROTON_WRAPPER_SHELL: ProtonWrapperShellBin = ProtonWrapperShellBin(include_bytes!("../proton-wrapper-shell.exe"));

static SHELL_NAME: &str = "proton-wrapper-shell.exe";

#[derive(Debug, Clone)]
pub struct MoutnedProtonWrapperShell {
    pub bin_path: PathBuf,
}

impl ProtonWrapperShellBin {
    pub fn mount(self, at: &Path) -> Result<MoutnedProtonWrapperShell> {
        at.join(SHELL_NAME).pipe(|bin_path| {
            std::fs::write(&bin_path, self.0)
                .context("injecting the binary")
                .map(|_| MoutnedProtonWrapperShell { bin_path })
        })
    }
}

#[derive(Debug, Clone)]
pub struct ProtonContext {
    pub proton_path: PathBuf,
    pub prefix_dir: Arc<TempDir>,
    pub steam_path: PathBuf,
    pub show_gui: bool,
}

pub trait CommandWrapInProtonExt {
    fn wrap_in_proton(&mut self, context: &Initialized<ProtonContext>) -> Result<WrappedCommand>;
}

impl CommandWrapInProtonExt for Command {
    fn wrap_in_proton(&mut self, context: &Initialized<ProtonContext>) -> Result<WrappedCommand> {
        context.wrap(self)
    }
}

#[derive(Debug)]
pub struct Initialized<T>(T, MoutnedProtonWrapperShell);

#[extension_traits::extension(pub trait CommandBetterOutputExt)]
impl Command {
    fn stdout_ok(&mut self) -> anyhow::Result<String> {
        let cmd_debug = format!("{self:?}");
        tracing::trace!("running command: [{cmd_debug}]");
        self.output()
            .context("spawning command failed")
            .and_then(|output| {
                let status = output.status;
                match status.success() {
                    true => Ok(String::from_utf8_lossy(&output.stdout).to_string()),
                    false => Err(anyhow!(
                        "status: {}\n\nstdout:\n{}\n\nstderr:\n{}",
                        output.status,
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    )),
                }
            })
            .with_context(|| format!("when running command: [{cmd_debug}]"))
    }
}

impl<T: Read> WrappedStdout<T> {
    pub fn read(self) -> Result<WrappedStdout<String>> {
        self.try_map(|mut path| {
            String::new().pipe(|mut out| {
                path.read_to_string(&mut out)
                    .context("reading to string")
                    .map(|_| out)
            })
        })
    }
}

impl<T: AsRef<Path>> WrappedStdout<T> {
    pub fn open(self) -> Result<WrappedStdout<File>> {
        self.try_map(|path| {
            let path = path.as_ref();
            std::fs::File::open(path).with_context(|| format!("opening [{path:?}]"))
        })
    }
}

impl<T: std::fmt::Display> std::fmt::Display for WrappedStdout<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.pipe(|Self { stdout, stderr }| write!(f, "stdout:\n{stdout}\n\nstderr:\n{stderr}"))
    }
}

impl WrappedCommand {
    #[instrument]
    pub fn output_blocking(mut self) -> Result<String> {
        debug!("running command: [{:?}]", self.serialized_command);

        self.wrapped_command
            .stdout_ok()
            .map(|out| debug!("{out}"))
            .and_then(|_| {
                std::fs::read_to_string(&self.wrapped_stdio.stdout)
                    .with_context(|| format!("reading stdout at [{}]", self.wrapped_stdio.stdout.display()))
                    .map(|all_output| {
                        debug!(%all_output);

                        #[cfg(debug_assertions)]
                        std::fs::write(self.context.prefix_dir.path().join("DUMP_STDOUT"), &all_output).expect("dumping output");

                        all_output
                            .lines()
                            .map(|l| l.trim())
                            .filter(|l| l.starts_with("proton_wrapper_shell:").not())
                            .join("\n")
                            .tap(|output| debug!("trimmed output:\n{output}"))
                    })
            })
            // .pipe(|res| match res {
            //     Ok(v) => Ok(v),
            //     Err(error) => Err(error).with_context,
            // })
            .with_context(|| format!("when running command: [{:#?}]", self.serialized_command))
            .with_context(|| format!("when running proton command command: {:?}", self.wrapped_command))
            .with_context(|| {
                self.wrapped_stdio
                    .clone()
                    .open()
                    .and_then(|opened| opened.read())
                    .map(|stdout| format!("{stdout}"))
                    .context("reading stdout due to error")
                    .unwrap_or_else(|fetching_original_stderr| format!("could not read process stdout failed:\n{fetching_original_stderr:?}"))
            })
            .with_context(|| {
                self.mounted_shell_wrapper
                    .bin_path
                    .parent()
                    .context("path has no parent")
                    .map(WrappedStdout::in_directory)
                    .and_then(|stdio| stdio.open().and_then(|s| s.read()))
                    .context("reading emergency stdio")
                    .map(|output| format!("wrapper crash:\n{output}"))
                    .unwrap_or_else(|fetching_emergency_stderr| format!("could not even read emergency stdio, reason:\n{fetching_emergency_stderr:?}"))
            })
    }
}

const WINE_HIDE_GUI_FLAGS: &str = "msdia80.dll=n";

impl ProtonContext {
    pub fn initialize_with_installs(self, installer_paths: &[(impl AsRef<Path>, &[&str])]) -> Result<Initialized<Self>> {
        self.initialize()
            .and_then(|context| {
                installer_paths
                    .iter()
                    .try_for_each(|(path, args)| {
                        path.as_ref()
                            .canonicalize()
                            .context("canonicalizing")
                            .and_then(|path| {
                                context
                                    .host_to_pfx_path(&path)
                                    .context("making path a pfx path")
                                    .and_then(|path| {
                                        info!("installing [{path:?}]");
                                        std::process::Command::new(path.as_path())
                                            .args(*args)
                                            .wrap_in_proton(&context)
                                            .and_then(|command| command.output_blocking().map(|_| ()))
                                    })
                                    .with_context(|| format!("installing [{path:?}]"))
                            })
                    })
                    .map(|_| context)
            })
            .tap_ok(|_| info!("[OK] proton context initialized"))
    }
    #[instrument]
    pub fn initialize(self) -> Result<Initialized<Self>> {
        debug!("initializing proton context");
        let Self {
            proton_path: _,
            prefix_dir,
            steam_path: _,
            show_gui,
        } = &self;
        PROTON_WRAPPER_SHELL
            .mount(prefix_dir.path())
            .context("mounting proton wrapper shell")
            .and_then(|mounted| {
                let mut command = Command::new("cmd.exe");
                command
                    .arg("/c")
                    .arg("echo")
                    .arg("TEST")
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit());
                let command = self
                    .wrap_inner(&mut command, &mounted)
                    .context("wrapping the command")?;
                debug!("running init command: [{command:?}]");
                command
                    .output_blocking()
                    .and_then(|output| {
                        debug!("output: {output}");
                        match output.as_str().trim().eq("TEST") {
                            true => Ok(()),
                            false => Err(anyhow!("expected 'TEST', found '{output}'")),
                        }
                    })
                    .with_context(|| format!("initializing proton context for {self:#?}"))
                    .map(|_| Initialized(self, mounted))
            })
    }
}

#[derive(Debug)]
pub struct WrappedCommand {
    #[allow(dead_code)]
    log_directory: TempDir,
    #[allow(dead_code)]
    context: ProtonContext,
    wrapped_command: Command,
    serialized_command: SerializedCommand,
    wrapped_stdio: WrappedStdout<PathBuf>,
    mounted_shell_wrapper: MoutnedProtonWrapperShell,
}

const APP_ID: &str = "proton-wrapper-logging";

fn make_fifo_pipe(at: PathBuf) -> Result<PathBuf> {
    nix::unistd::mkfifo(&at, nix::sys::stat::Mode::S_IRWXU)
        .context("creating pipe for stdout")
        .with_context(|| format!("creating fifo pipe at {at:?}"))
        .map(|_| at)
}

impl ProtonContext {
    fn wrap_inner(&self, command: &mut Command, ipc: &MoutnedProtonWrapperShell) -> Result<WrappedCommand> {
        let Self {
            proton_path,
            prefix_dir,
            steam_path,
            show_gui,
        } = self;
        debug!("wrapping command [{command:?}]");
        let mut wrapped = Command::new(proton_path);

        let log_directory = tempfile::Builder::new()
            .prefix("log_directory")
            .tempdir_in(prefix_dir.path())
            .context("creating temporary log directory")?;

        let wrapped_stdio = WrappedStdout::in_directory(log_directory.path());
        let serialized_command = SerializedCommand::from_command(
            command,
            wrapped_stdio
                .clone()
                .try_map(|path| host_to_pfx_path(&path))
                .map(|paths| paths.map(|p| p.to_string()))?,
        );

        wrapped
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .arg("run")
            .arg(
                ipc.bin_path
                    .pipe_deref(host_to_pfx_path)
                    .context("converting binary name to host path")?,
            )
            .arg(
                serialized_command
                    .serialize()
                    .context("seriallizing command")?,
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .pipe(|c| match show_gui {
                true => c,
                false => c.env("WINEDLLOVERRIDES", WINE_HIDE_GUI_FLAGS),
            })
            // .arg(wrapped_command)
            // .envs(command.get_envs().filter_map(|(k, v)| v.map(|v| (k, v))))
            // .env("PROTON_LOG", "1")
            // .env("PROTON_LOG_DIR", log_directory.path())
            .env("STEAM_COMPAT_DATA_PATH", prefix_dir.path())
            .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_path)
            .env("SteamGameId", APP_ID);

        if let Some(current_dir) = command.get_current_dir() {
            wrapped.current_dir(current_dir);
        }

        Ok(WrappedCommand {
            context: self.clone(),
            wrapped_command: wrapped,
            serialized_command,
            wrapped_stdio,
            log_directory,
            mounted_shell_wrapper: ipc.clone(),
        })
    }
}

pub fn host_to_pfx_path(path: &Path) -> Result<Utf8WindowsPathBuf> {
    const ROOT: &str = "Z:\\";
    Utf8UnixPath::new(&path.to_string_lossy())
        .normalize()
        .absolutize()
        .context("could not make path absolute")
        .and_then(|path| {
            path.with_windows_encoding_checked()
                .context("converting stdout to windows encofing")
        })
        .and_then(|absolute| {
            absolute
                .components()
                .filter_map(|e| match e {
                    typed_path::Utf8WindowsComponent::Normal(normal) => Some(normal),
                    _ => None,
                })
                .try_fold(Utf8WindowsPathBuf::new(), |acc, next| {
                    acc.join_checked(next)
                        .with_context(|| format!("extending {acc} with {next}"))
                })
                .and_then(|relative| {
                    Utf8WindowsPath::new(ROOT)
                        .join_checked(relative)
                        .with_context(|| format!("prefixing path with '{ROOT}'"))
                })
        })
        .with_context(|| format!("translating [{path:?}] to a path inside the prefix (assumming [{ROOT}])"))
}

impl ProtonContext {
    pub fn host_to_pfx_path(&self, path: &Path) -> Result<Utf8WindowsPathBuf> {
        host_to_pfx_path(path)
    }
}

impl Initialized<ProtonContext> {
    pub fn wrap(&self, command: &mut Command) -> Result<WrappedCommand> {
        self.0.wrap_inner(command, &self.1)
    }
    pub fn host_to_pfx_path(&self, path: &Path) -> Result<Utf8WindowsPathBuf> {
        self.0.host_to_pfx_path(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test_log::test]
    fn test_it_works() -> Result<()> {
        debug!("testing if it works");
        ProtonContext {
            proton_path: "/home/niedzwiedz/.local/share/Steam/steamapps/common/Proton - Experimental/proton".into(),
            prefix_dir: Arc::new(TempDir::new()?),
            steam_path: "/home/niedzwiedz/.local/share/Steam".into(),
            show_gui: false,
        }
        .initialize()
        .and_then(|c| {
            Command::new("cmd.exe")
                .arg("/c")
                .arg(r#"echo ACTUAL TEST"#)
                .wrap_in_proton(&c)
                .and_then(|c| c.output_blocking())
                .and_then(|o| match o.trim().eq("ACTUAL TEST") {
                    true => Ok(()),
                    false => anyhow::bail!("expected 'TEST', found '{o}'"),
                })
        })
    }
}
