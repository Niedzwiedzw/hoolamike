#![allow(clippy::unit_arg)]
#![feature(seek_stream_len)]
#![feature(slice_take)]
use {
    anyhow::{Context, Result},
    clap::{Args, Parser, Subcommand, ValueEnum},
    modlist_data::ModlistSummary,
    modlist_json::DirectiveKind,
    num::ToPrimitive,
    std::{ops::Div, path::PathBuf, str::FromStr},
    tap::{Pipe, TapFallible},
};
pub const BUFFER_SIZE: usize = 1024 * 64;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    /// the hoolamike config file is where you configure your installation - we're linux users, we can't afford windows
    /// which means we can't afford GUI-capable hardware anyway
    ///
    /// in the config you'll have to specify a modlist file - you'll have to download it
    /// can it be downloaded autside of wabbajack gui client?
    /// yes and no
    /// they can be found here: https://build.wabbajack.org/authored_files **BUT** the manual download should be avoided unless absolutely necessary.
    /// probably best approach would be visiting official Wabbajack discord server and asking someone which file is safe to download
    #[arg(long, short = 'c', default_value = std::env::current_dir().unwrap().join("hoolamike.yaml").into_os_string())]
    hoolamike_config: PathBuf,
    #[command(subcommand)]
    command: Commands,
    /// generates a flamegraph, useful for performance testing (SLOW!)
    #[arg(long, value_enum, default_value_t = Default::default())]
    logging_mode: LoggingMode,
}

#[derive(clap::Args, Default)]
pub struct DebugHelpers {
    /// skip verification (used mostly for developing the tool)
    #[arg(long)]
    skip_verify_and_downloads: bool,
    #[arg(long)]
    start_from_directive: Option<String>,
    #[arg(long)]
    skip_kind: Vec<DirectiveKind>,
    #[arg(long)]
    contains: Vec<String>,
}

#[derive(Subcommand)]
enum HoolamikeDebugCommand {
    ReserializeDirectives { modlist_file: PathBuf },
}

#[derive(Args)]
struct HoolamikeDebug {
    #[command(subcommand)]
    command: HoolamikeDebugCommand,
}

#[derive(Subcommand)]
enum Commands {
    HoolamikeDebug(HoolamikeDebug),
    /// tests the modlist parser
    ValidateModlist {
        /// path to modlist (.wabbajack) file
        path: PathBuf,
    },
    /// prints information about the modlist
    ModlistInfo {
        /// path to modlist (.wabbajack) file
        path: PathBuf,
    },
    Install {
        #[command(flatten)]
        debug: DebugHelpers,
    },
    /// prints prints default config. save it and modify to your liking
    PrintDefaultConfig,
    /// runs post-install fixup - wouldn't be possible without extensive research done by Omni
    /// make sure to star his repo: https://github.com/Omni-guides/Wabbajack-Modlist-Linux
    PostInstallFixup,
}

pub mod read_wrappers;
#[macro_use]
pub mod utils;

pub mod compression;
pub mod config_file;
pub mod downloaders;
pub mod error;
pub mod helpers;
pub mod install_modlist;
pub mod modlist_data;
pub mod modlist_json;
pub mod octadiff_reader;
pub mod post_install_fixup;
pub mod progress_bars_v2;
pub mod wabbajack_file;
pub mod consts {
    use {once_cell::sync::Lazy, std::path::Path, tap::prelude::*};
    pub static TEMP_FILE_DIR: Lazy<&'static Path> =
        Lazy::new(|| Path::new(".hoolamike/TEMP_FILES").tap(|path| std::fs::create_dir_all(path).expect("could not create temporary dir storage")));
}

#[derive(Debug, ValueEnum, Clone, Copy, Default, serde::Serialize)]
pub enum LoggingMode {
    #[default]
    Cli,
    Flamegraph,
    TracingConsole,
}

#[allow(unused_imports)]
fn setup_logging(logging_mode: LoggingMode) -> Option<impl Drop> {
    use {
        tracing_indicatif::IndicatifLayer,
        tracing_subscriber::{fmt, layer::SubscriberExt, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter},
    };
    match logging_mode {
        LoggingMode::Flamegraph => {
            let fmt_layer = fmt::Layer::default();

            let (flame_layer, guard) = tracing_flame::FlameLayer::with_file("./tracing.folded").unwrap();

            let subscriber = tracing_subscriber::Registry::default()
                .with(fmt_layer)
                .with(flame_layer);

            tracing::subscriber::set_global_default(subscriber).expect("Could not set global default");
            Some(guard)
        }
        LoggingMode::Cli => {
            let indicatif_layer = console::Term::stdout()
                .size_checked()
                .map(|(_width, height)| height)
                .and_then(|v| v.to_u64())
                .unwrap_or(50)
                .div(2)
                .pipe(|half_height| {
                    IndicatifLayer::new()
                        .with_progress_style(
                            indicatif::ProgressStyle::with_template("{span_child_prefix:.bold}▕({elapsed:.yellow}) {span_name:.blue}({span_fields:.yellow})")
                                .expect("bad progress style"),
                        )
                        .with_max_progress_bars(
                            half_height,
                            Some(indicatif::ProgressStyle::with_template("...and {pending_progress_bars} more not shown above.").unwrap()),
                        )
                });
            // let indicatif_layer = ;
            let subscriber = tracing_subscriber::registry()
                .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::from_str("info").unwrap()))
                .with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
                .with(indicatif_layer);
            tracing::subscriber::set_global_default(subscriber)
                .context("Unable to set a global subscriber")
                .expect("logging failed");
            None
        }
        LoggingMode::TracingConsole => {
            use tracing_subscriber::prelude::*;

            // spawn the console server in the background,
            // returning a `Layer`:
            let console_layer = console_subscriber::spawn();

            // build a `Subscriber` by combining layers with a
            // `tracing_subscriber::Registry`:
            tracing_subscriber::registry()
                // add the console layer to the subscriber
                .with(console_layer)
                // add other layers...
                .with(tracing_subscriber::fmt::layer())
                // .with(...)
                .init();
            None
        }
    }
}

async fn async_main() -> Result<()> {
    let Cli {
        command,
        hoolamike_config,
        logging_mode,
    } = Cli::parse();
    let _guard = setup_logging(logging_mode);

    match command {
        Commands::PostInstallFixup => {
            let (_config_path, config) = config_file::HoolamikeConfig::find(&hoolamike_config).context("reading hoolamike config file")?;
            post_install_fixup::run_post_install_fixup(&config)
        }
        Commands::ValidateModlist { path } => tokio::fs::read_to_string(&path)
            .await
            .context("reading test file")
            .and_then(|input| modlist_json::parsing_helpers::validate_modlist_file(&input))
            .with_context(|| format!("testing file {}", path.display())),
        Commands::ModlistInfo { path } => wabbajack_file::WabbajackFile::load_wabbajack_file(path)
            .context("reading modlist")
            .map(|(_, modlist)| ModlistSummary::new(&modlist.modlist))
            .map(|modlist| modlist.print())
            .map(|modlist| println!("\n{modlist}")),
        Commands::PrintDefaultConfig => config_file::HoolamikeConfig::default()
            .write()
            .map(|config| println!("{config}")),
        Commands::Install { debug } => {
            let (config_path, config) = config_file::HoolamikeConfig::find(&hoolamike_config).context("reading hoolamike config file")?;
            tracing::info!("found config at [{}]", config_path.display());

            install_modlist::install_modlist(config, debug)
                .await
                .map_err(|errors| {
                    errors
                        .iter()
                        .enumerate()
                        .for_each(|(idx, reason)| tracing::error!("{idx}. {reason:?}", idx = idx + 1));

                    anyhow::anyhow!("could not finish installation due to [{}] errors", errors.len())
                })
                .map(|count| println!("successfully installed [{}] mods", count.len()))
        }
        Commands::HoolamikeDebug(HoolamikeDebug { command }) => match command {
            HoolamikeDebugCommand::ReserializeDirectives { modlist_file } => wabbajack_file::WabbajackFile::load_wabbajack_file(modlist_file)
                .context("loading modlist file")
                .and_then(|modlist| {
                    modlist
                        .1
                        .modlist
                        .directives
                        .pipe_ref(|directives| serde_json::to_string_pretty(directives).context("serializing directives"))
                })
                .map(|directives| println!("{directives}")),
        },
    }
    .with_context(|| {
        format!(
            "\n\nerror occurred, run with --help, check your configuration or file a ticket at {}",
            env!("CARGO_PKG_REPOSITORY")
        )
    })
    .tap_err(|e| {
        tracing::error!("\n\n{e:?}");
    })
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(num_cpus::get().saturating_sub(2).max(1))
        .build_global()
        .unwrap();
    async_main().await
}
