#![allow(dead_code)]

use {
    anyhow::{Context, Result},
    binrw::io::BufReader,
    gag::Redirect,
    std::io::{PipeReader, PipeWriter},
};

pub enum Message {}

pub struct EmbeddedTerminalState {
    handles: [Redirect<PipeWriter>; 2],
    stdout: BufReader<PipeReader>,
    stderr: BufReader<PipeReader>,
}

impl EmbeddedTerminalState {
    pub fn new() -> Result<Self> {
        let (stdout, stdout_handle) = {
            std::io::pipe()
                .context("creating a pipe")
                .and_then(|(rx, tx)| {
                    gag::Redirect::stdout(tx)
                        .context("creating redirect")
                        .map(|handle| (BufReader::new(rx), handle))
                })
                .context("redirecting stdout")
        }?;
        let (stderr, stderr_handle) = {
            std::io::pipe()
                .context("creating a pipe")
                .and_then(|(rx, tx)| {
                    gag::Redirect::stderr(tx)
                        .context("creating redirect")
                        .map(|handle| (BufReader::new(rx), handle))
                })
                .context("redirecting stderr")
        }?;

        Ok(Self {
            handles: [stdout_handle, stderr_handle],
            stdout,
            stderr,
        })
    }
}
