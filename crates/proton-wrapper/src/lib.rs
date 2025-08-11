use {
    anyhow::{anyhow, Context, Result},
    itertools::Itertools,
    std::{
        fs::File,
        io::{BufRead, BufReader},
        path::{Path, PathBuf},
        process::{Command, Stdio},
    },
    tap::{Pipe, Tap, TapFallible},
    tempfile::TempDir,
    tracing::{debug, info, instrument},
    typed_path::{Utf8UnixPath, Utf8WindowsPath, Utf8WindowsPathBuf},
};

#[derive(Debug, Clone)]
pub struct ProtonContext {
    pub proton_path: PathBuf,
    pub prefix_dir: PathBuf,
    pub steam_path: PathBuf,
}

pub trait CommandWrapInProtonExt {
    fn wrap_in_proton(self, context: &Initialized<ProtonContext>) -> Result<WrappedCommand>;
}

impl CommandWrapInProtonExt for Command {
    fn wrap_in_proton(self, context: &Initialized<ProtonContext>) -> Result<WrappedCommand> {
        context.wrap(self)
    }
}

#[derive(Debug)]
pub struct Initialized<T>(T);

#[extension_traits::extension(pub trait CommandBetterOutputExt)]
impl Command {
    fn stdout_ok(mut self) -> anyhow::Result<String> {
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

impl WrappedCommand {
    pub fn output(mut self) -> Result<String> {
        self.wrapped_command
            .spawn()
            .context("spawning command")
            .and_then(|spawned| {
                self.log_stream
                    .open()
                    .and_then(|opened| {
                        debug!("named pipe opened");
                        opened
                            .stdout
                            .lines()
                            .map(|line| {
                                line.context("bad line").map(|line| {
                                    debug!("[stdout] {line}");
                                    line
                                })
                            })
                            .collect::<Result<Vec<String>>>()
                    })
                    .and_then(|stdout| {
                        spawned
                            .wait_with_output()
                            .context("waiting for command output")
                            .and_then(|output| match output.status.success() {
                                true => Ok(output.status),
                                false => Err(anyhow!("bad status: {}", output.status)),
                            })
                            .map(|_| stdout.join("\n"))
                    })
            })
    }
}

impl ProtonContext {
    #[instrument]
    pub fn initialize(self) -> Result<Initialized<Self>> {
        debug!("initializing proton context");
        let Self {
            proton_path: _,
            prefix_dir,
            steam_path: _,
        } = &self;
        if !prefix_dir.exists() {
            debug!("creating pfx directory");
            std::fs::create_dir_all(prefix_dir).with_context(|| format!("creating prefix for [{}]", prefix_dir.display()))?;
        }
        let mut command = Command::new("echo");

        command
            .args(["TEST"])
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let command = self.wrap_inner(command).context("wrapping the command")?;
        debug!("running init command: [{command:?}]");
        command
            .output()
            .and_then(|output| {
                debug!("output: {output}");
                match output.as_str().trim().eq("TEST") {
                    true => Ok(()),
                    false => Err(anyhow!("expected 'TEST', found '{output}'")),
                }
            })
            .with_context(|| format!("initializing proton context for {self:#?}"))
            .map(|_| Initialized(self))
    }
}

#[derive(Debug)]
pub struct WrappedCommand {
    #[allow(dead_code)]
    context: ProtonContext,
    wrapped_command: Command,
    log_stream: StdoutStream,
}

const APP_ID: &str = "proton-wrapper-logging";

#[derive(Debug)]
pub struct StdoutStream {
    temp_dir: TempDir,
    stdout: PathBuf,
}

pub struct OpenedStdoutStream {
    #[allow(dead_code)]
    temp_dir: TempDir,
    stdout: BufReader<File>,
}
impl StdoutStream {
    #[instrument]
    pub fn open(self) -> Result<OpenedStdoutStream> {
        debug!("log task is spawning and awaiting for writes");
        self.pipe(|Self { stdout, temp_dir }| {
            let open = |file: &Path| {
                std::fs::File::options()
                    .read(true)
                    .open(file)
                    .with_context(|| format!("opening log stream: {file:?}"))
                    .map(BufReader::new)
            };
            Ok(OpenedStdoutStream {
                temp_dir,
                stdout: open(&stdout)?,
            })
        })
    }
}

fn make_fifo_pipe(at: PathBuf) -> Result<PathBuf> {
    nix::unistd::mkfifo(&at, nix::sys::stat::Mode::S_IRWXU)
        .context("creating pipe for stdout")
        .with_context(|| format!("creating fifo pipe at {at:?}"))
        .map(|_| at)
}

impl ProtonContext {
    fn wrap_inner(&self, command: Command) -> Result<WrappedCommand> {
        let Self {
            proton_path,
            prefix_dir,
            steam_path,
        } = self;
        debug!("wrapping command [{command:?}]");
        let mut wrapped = Command::new(proton_path);

        let absolute_in_prefix = |path: &Path| self.host_to_pfx_path(path);

        let log_directory = TempDir::new_in(prefix_dir).context("creating temporary log directory")?;
        let stdout = log_directory
            .path()
            .join("stdout.txt")
            .pipe(make_fifo_pipe)?;

        fn double_quote(s: &str) -> String {
            ["\"", s, "\""].join("")
        }
        let wrapped_command = {
            let stdout = stdout.pipe_ref(|p| absolute_in_prefix(p).map(|o| o.to_string().pipe_deref(double_quote)))?;

            format!(
                "{program} {params} >{stdout} 2>&1",
                program = command.get_program().to_string_lossy(),
                params = command
                    .get_args()
                    .map(|a| {
                        //
                        a.to_string_lossy()
                        // .pipe_deref(escape)
                        // .pipe_deref(single_quote)
                    })
                    .join(" "),
                stdout = stdout,
            )
            .tap(|escaped| info!("escaped command: [{escaped}]"))
        };

        const COMMAND_BAT: &str = "command.bat";
        let bat_file = stdout
            .parent()
            .context("must have a parent")
            .and_then(|parent| absolute_in_prefix(parent).map(|p| double_quote(p.as_str())))
            .map(|parent| format!("@echo off\nif not exist {parent} mkdir {parent}\n{wrapped_command}",))
            .tap_ok(|bat_file_contents| debug!("bat file contents:\n```\n{bat_file_contents}\n```"))
            .and_then(|bat_file_contents| {
                prefix_dir.join(COMMAND_BAT).pipe(|command_bat| {
                    std::fs::write(&command_bat, bat_file_contents)
                        .with_context(|| format!("writing bat file to {command_bat:?}"))
                        .map(|_| command_bat)
                })
            })
            .and_then(|bat_file| absolute_in_prefix(&bat_file))
            .context("creating bat file with encoded command")?;
        wrapped
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .arg("run")
            .arg("cmd.exe")
            .arg("/c")
            .arg(bat_file)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // .arg(wrapped_command)
            // .envs(command.get_envs().filter_map(|(k, v)| v.map(|v| (k, v))))
            // .env("PROTON_LOG", "1")
            // .env("PROTON_LOG_DIR", log_directory.path())
            .env("STEAM_COMPAT_DATA_PATH", prefix_dir)
            .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_path)
            .env("SteamGameId", APP_ID);

        if let Some(current_dir) = command.get_current_dir() {
            wrapped.current_dir(current_dir);
        }

        Ok(WrappedCommand {
            context: self.clone(),
            wrapped_command: wrapped,
            log_stream: StdoutStream {
                stdout,
                temp_dir: log_directory,
            },
        })
    }
}

impl ProtonContext {
    pub fn host_to_pfx_path(&self, path: &Path) -> Result<Utf8WindowsPathBuf> {
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
}

impl Initialized<ProtonContext> {
    pub fn wrap(&self, command: Command) -> Result<WrappedCommand> {
        self.0.wrap_inner(command)
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
            prefix_dir: "/tmp/test-pfx".into(),
            steam_path: "/home/niedzwiedz/.local/share/Steam".into(),
        }
        .initialize()
        .map(|_| ())
    }
}
