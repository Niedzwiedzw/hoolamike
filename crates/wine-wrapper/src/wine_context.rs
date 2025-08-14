use {
    crate::ipc::{SerializedCommand, WineWrapperShellBin, WrappedStdout},
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

// static WINE_WRAPPER_SHELL: WineWrapperShellBin = WineWrapperShellBin(include_bytes!("../wine-wrapper-shell.exe"));
static WINE_WRAPPER_SHELL: WineWrapperShellBin = WineWrapperShellBin(&[]);

static SHELL_NAME: &str = "wine-wrapper-shell.exe";

#[derive(Debug, Clone)]
pub struct MoutnedWineWrapperShell {
    pub bin_path: PathBuf,
}

impl WineWrapperShellBin {
    pub fn mount(self, at: &Path) -> Result<MoutnedWineWrapperShell> {
        at.join(SHELL_NAME).pipe(|bin_path| {
            std::fs::write(&bin_path, self.0)
                .context("injecting the binary")
                .map(|_| MoutnedWineWrapperShell { bin_path })
        })
    }
}

#[derive(Debug, Clone)]
pub struct WineContext {
    pub wine_path: PathBuf,
    pub prefix_dir: Arc<TempDir>,
    pub show_gui: bool,
}

pub trait CommandWrapInWineExt {
    fn wrap_in_wine(&mut self, context: &Initialized<WineContext>) -> Result<WrappedCommand>;
}

impl CommandWrapInWineExt for Command {
    fn wrap_in_wine(&mut self, context: &Initialized<WineContext>) -> Result<WrappedCommand> {
        context.wrap(self)
    }
}

#[derive(Debug)]
pub struct Initialized<T>(T, MoutnedWineWrapperShell);

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
                            .filter(|l| l.starts_with("wine_wrapper_shell:").not())
                            .join("\n")
                            .tap(|output| debug!("trimmed output:\n{output}"))
                    })
            })
            // .pipe(|res| match res {
            //     Ok(v) => Ok(v),
            //     Err(error) => Err(error).with_context,
            // })
            .with_context(|| format!("when running command: [{:#?}]", self.serialized_command))
            .with_context(|| format!("when running wine command command: {:?}", self.wrapped_command))
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

impl WineContext {
    #[instrument(skip_all)]
    pub fn wait_wineserver_idle(&self) -> Result<()> {
        debug!("waiting");
        std::process::Command::new("wineserver")
            .arg("-w")
            .stdout_ok()
            .map(|_| {
                debug!("[OK] idle");
            })
    }
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
                                            .wrap_in_wine(&context)
                                            .and_then(|command| command.output_blocking().map(|_| ()))
                                            .and_then(|_| context.0.wait_wineserver_idle())
                                    })
                                    .with_context(|| format!("installing [{path:?}]"))
                            })
                    })
                    .map(|_| context)
            })
            .tap_ok(|_| info!("[OK] wine context initialized"))
    }
    #[instrument]
    pub fn initialize(self) -> Result<Initialized<Self>> {
        debug!("initializing wine context");
        std::thread::sleep(std::time::Duration::from_millis(1000));

        let Self {
            wine_path: _,
            prefix_dir,
            show_gui: _,
        } = &self;
        WINE_WRAPPER_SHELL
            .mount(prefix_dir.path())
            .context("mounting wine wrapper shell")
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
                    .with_context(|| format!("initializing wine context for {self:#?}"))
                    .map(|_| Initialized(self, mounted))
            })
    }
}

#[derive(Debug)]
pub struct WrappedCommand {
    #[allow(dead_code)]
    log_directory: TempDir,
    #[allow(dead_code)]
    context: WineContext,
    wrapped_command: Command,
    serialized_command: SerializedCommand,
    wrapped_stdio: WrappedStdout<PathBuf>,
    mounted_shell_wrapper: MoutnedWineWrapperShell,
}

const APP_ID: &str = "wine-wrapper-logging";

#[allow(dead_code)]
fn make_fifo_pipe(at: PathBuf) -> Result<PathBuf> {
    nix::unistd::mkfifo(&at, nix::sys::stat::Mode::S_IRWXU)
        .context("creating pipe for stdout")
        .with_context(|| format!("creating fifo pipe at {at:?}"))
        .map(|_| at)
}

impl WineContext {
    fn wrap_inner(&self, command: &mut Command, ipc: &MoutnedWineWrapperShell) -> Result<WrappedCommand> {
        let Self {
            wine_path: _,
            prefix_dir,
            show_gui,
        } = self;
        debug!("wrapping command [{command:?}]");
        // let mut wrapped = Command::new(wine_path);
        let mut wrapped = Command::new("wine");

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
            // .arg("run")
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
            .env("WINEPREFIX", prefix_dir.path())
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

impl WineContext {
    pub fn host_to_pfx_path(&self, path: &Path) -> Result<Utf8WindowsPathBuf> {
        host_to_pfx_path(path)
    }
}

impl Initialized<WineContext> {
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
        WineContext {
            wine_path: "wine".into(),
            prefix_dir: Arc::new(
                tempfile::Builder::new()
                    .prefix("pfx-")
                    .tempdir_in(env!("CARGO_MANIFEST_DIR"))?,
            ),
            show_gui: false,
        }
        .initialize()
        .and_then(|c| {
            Command::new("cmd.exe")
                .arg("/c")
                .arg(r#"echo ACTUAL TEST"#)
                .wrap_in_wine(&c)
                .and_then(|c| c.output_blocking())
                .and_then(|o| match o.trim().eq("ACTUAL TEST") {
                    true => Ok(()),
                    false => anyhow::bail!("expected 'TEST', found '{o}'"),
                })
        })
    }
}
