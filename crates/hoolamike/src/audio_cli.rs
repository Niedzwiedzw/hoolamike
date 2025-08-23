#[derive(clap::Args, Clone)]
pub struct AudioCliCommand {
    #[command(subcommand)]
    pub command: hoola_audio::Commands,
}
