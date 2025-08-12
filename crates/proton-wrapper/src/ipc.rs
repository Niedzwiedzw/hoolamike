use {
    anyhow::{Context, Result},
    base64::prelude::*,
    serde::{Deserialize, Serialize},
    std::{
        path::{Path, PathBuf},
        str::FromStr,
    },
    tap::{Pipe, Tap},
};
pub use {base64, serde_json};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WrappedStdout<T> {
    pub stdout: T,
    pub stderr: T,
}

impl<T> WrappedStdout<T> {
    pub fn try_map<U>(self, mut map: impl FnMut(T) -> Result<U>) -> Result<WrappedStdout<U>> {
        Ok(WrappedStdout {
            stdout: map(self.stdout).context("wrapping stdout")?,
            stderr: map(self.stderr).context("mapping stderr")?,
        })
    }
    pub fn map<U>(self, mut map: impl FnMut(T) -> U) -> WrappedStdout<U> {
        WrappedStdout {
            stdout: map(self.stdout),
            stderr: map(self.stderr),
        }
    }
}

impl WrappedStdout<PathBuf> {
    pub fn in_directory(directory: &Path) -> Self {
        Self {
            stdout: directory.join("stdout"),
            stderr: directory.join("stderr"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ProtonWrapperShellBin(pub &'static [u8]);

impl std::fmt::Debug for ProtonWrapperShellBin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProtonWrapperShellBin")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SerializedCommand {
    pub bin: PathBuf,
    pub args: Vec<String>,
    pub stdio: WrappedStdout<String>,
}

impl SerializedCommand {
    pub fn from_command(command: &std::process::Command, stdio: WrappedStdout<String>) -> Self {
        SerializedCommand {
            bin: command.get_program().pipe(PathBuf::from),
            args: command
                .get_args()
                .map(|arg| arg.to_string_lossy().to_string())
                .collect(),
            stdio,
        }
    }

    pub fn to_command(&self) -> std::process::Command {
        std::process::Command::new(&self.bin).tap_mut(|c| {
            c.args(&self.args);
        })
    }

    pub fn decode(s: &str) -> Result<Self> {
        BASE64_STANDARD
            .decode(s)
            .context("decoding command")
            .and_then(|command| String::from_utf8(command).context("not utf8"))
            .and_then(|command| serde_json::from_str::<Self>(&command).context("decoding json"))
    }
}

impl FromStr for SerializedCommand {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::decode(s)
    }
}

impl SerializedCommand {
    pub fn serialize(&self) -> Result<String> {
        serde_json::to_string_pretty(&self)
            .context("serializing to json")
            .map(|s| BASE64_STANDARD.encode(&s))
    }
}
