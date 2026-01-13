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
}

pub fn parse_args() -> Args {
    let mut args = Args::parse();
    if args.bind.is_empty() && args.unix_bind.is_empty() {
        tracing::warn!("Neither --bind nor --unix-bind given - using default");
        args.bind.push("127.0.0.1:5000".into());
    }
    args
}
