use anyhow::Context as _;
use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    /// Number of workers
    #[clap(short, long, default_value_t = 2)]
    pub(crate) workers: usize,

    /// TCP Address(es) to bind to
    #[clap(short, long)]
    pub(crate) bind: Vec<String>,

    /// Unix socket(s) to bind to
    #[clap(short, long)]
    pub(crate) unix_bind: Vec<String>,

    /// Config path
    #[clap(short, long, default_value = "config.toml")]
    config_path: String,
}

pub fn parse_args() -> Args {
    let mut args = Args::parse();
    if args.bind.is_empty() && args.unix_bind.is_empty() {
        tracing::warn!("Neither --bind nor --unix-bind given - using default");
        args.bind.push("127.0.0.1:5000".into());
    }
    args
}

#[derive(serde::Deserialize, Debug)]
pub struct Config<T> {
    #[serde(flatten)]
    inner: T,
}

/// Loads the config from specified path
#[tracing::instrument(err)]
pub fn load_config<T>(args: &Args) -> anyhow::Result<T>
where
    T: serde::de::DeserializeOwned + std::fmt::Debug,
{
    let filepath = &args.config_path;
    tracing::info!("Trying to loading file: {filepath}");
    let conf: Config<T> = if let Ok(content) = fs_err::read_to_string(filepath) {
        toml::from_str(&content)
            .with_context(|| format!("Failed to toml load from '{filepath}'"))?
    } else {
        tracing::warn!("no config file found! Using default config");
        toml::from_str("").context("Failed to parse empty string as config")?
    };
    tracing::info!("Loaded config: {conf:?}");
    Ok(conf.inner)
}
