use {crate::modlist_json::HumanUrl, clap::Args};

#[derive(Args, Clone)]
pub struct HandleNxmCli {
    /// default port to listen to, preferrably set it through env
    /// WARN: it must be the same port for both instances
    #[arg(long, env, default_value_t = super::single_instance_server::DEFAULT_PORT)]
    pub port: u16,
    /// use this if you want to set up the nxm handler manually
    #[arg(long)]
    pub skip_nxm_register: bool,
    /// this is just a detail of the link handling protocol
    /// it should be included in the command that your system's dispatcher is gonna
    /// run
    #[arg(value_name = "URL")]
    pub nxm_link: Option<HumanUrl>,
    /// it will be invoked as <use-browser> <url>
    #[arg(long, default_value = "firefox")]
    pub use_browser: String,
}
