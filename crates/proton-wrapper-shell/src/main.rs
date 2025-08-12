use {
    anyhow::{Context, Result},
    clap::{Parser, Subcommand},
    proton_wrapper::ipc::{SerializedCommand, WrappedStdout},
    std::{
        fs::File,
        ops::Not,
        path::{Path, PathBuf},
        process::Stdio,
    },
    tap::{Pipe, Tap},
    tracing::{info, info_span},
};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// base64 command to run
    command: SerializedCommand,
    /// disable the file redirection of standard output
    #[arg(long)]
    no_redirect: bool,
}

fn create_parent_path(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creating parent path at [{parent:?}]")?;
    }
    Ok(())
}

pub fn main() -> Result<()> {
    let Cli { command, no_redirect } = Cli::parse();
    tracing_subscriber::fmt()
        .without_time()
        .with_level(false)
        .with_ansi(no_redirect)
        .init();
    let open_files = |w: WrappedStdout<_>| {
        w.try_map(|path| {
            std::fs::File::options()
                .truncate(true)
                .read(true)
                .create(true)
                .write(true)
                .open(&path)
                .with_context(|| format!("opening file at [{path:?}]"))
        })
        .context("opening stdout files for redirect")
    };
    let redirect_stdout = |w: WrappedStdout<File>| {
        no_redirect
            .not()
            .then(|| {
                w.pipe(|WrappedStdout { stdout, stderr }| {
                    Ok(WrappedStdout {
                        stdout: gag::Redirect::stdout(stdout).context("creating redirect for stdout")?,
                        stderr: gag::Redirect::stderr(stderr).context("creating redirect for stderr")?,
                    })
                })
            })
            .transpose()
    };
    let _wrapped_stdout_guard = {
        info!(?no_redirect, "setting up redirection");
        create_parent_path(command.stdio.stdout.as_str().pipe(Path::new))?;

        command
            .stdio
            .clone()
            .map(PathBuf::from)
            .pipe(open_files)
            .and_then(redirect_stdout)
            .unwrap_or_else(|reason| {
                std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .pipe_ref(|exe_dir| {
                        WrappedStdout::in_directory(exe_dir)
                            .pipe(open_files)
                            .and_then(redirect_stdout)
                    })
                    .unwrap()
                    .tap(|_| tracing::error!("COULD NOT SETUP PROVIDED STDOUT:\n{reason:?}\ncommand:\n{command:#?}"))
            })
    };
    info!("received command: {command:?}");
    let mut exit_status = None;
    command
        .to_command()
        .tap(|command| {
            info!("interpreted command as {command:?}");
        })
        .tap_mut(|c| {
            c.stdin(Stdio::inherit()).stdout(Stdio::inherit());
        })
        .spawn()
        .context("spawning command")
        .and_then(|mut child| {
            info!("command is running");
            child
                .wait()
                .context("waiting for command to finish")
                .and_then(|status| {
                    info!("{status}");
                    exit_status = Some(status);
                    match status.success() {
                        true => Ok(()),
                        false => Err(anyhow::anyhow!("BAD STATUS: {status}")),
                    }
                })
        })
        .with_context(|| format!("when running command:\n{command:?}"))
        .tap(|res| match res {
            Ok(()) => info!("SUCCESS"),
            Err(error) => tracing::error!("{error:?}"),
        })
        .tap(|_| {
            if let Some(status) = exit_status.and_then(|status| status.code()) {
                std::process::exit(status)
            }
        })
}
