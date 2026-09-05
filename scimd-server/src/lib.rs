//! The standalone SCIM daemon: sockets, workers, signals, systemd readiness, `--flags`, a config
//! file, a logging subscriber and the `/metrics` exposition, so that mounting [`scimd`] into a host
//! that already runs its own `HttpServer` costs none of it.
//!
//! ```no_run
//! # async fn f() -> anyhow::Result<()> {
//! # struct UserService;
//! # impl scimd::ResourceService<scimd::models::User> for UserService {}
//! scimd_server::init_logging()?;
//! let args = scimd_server::parse_args();
//!
//! let core = scimd::ScimBuilder::new().register::<scimd::models::User, _>(UserService);
//! let app = scimd::AppBuilder::new(core).build();
//! scimd_server::Server::new(app).args(args).run().await
//! # }
//! ```

use actix_web::HttpServer;
use tracing_subscriber::layer::SubscriberExt as _;

mod metrics;
mod opts;

pub use opts::{Args, load_config, parse_args};
/// Re-exported so a binary needs one dependency line rather than two kept in step.
pub use scimd;
use scimd::{App, MetricsRegistry};

const DEFAULT_BIND: &str = "127.0.0.1:8080";

/// Installs the process-wide `tracing` subscriber and routes the `log` crate into it.
///
/// A host embedding scimd does not call this - it has a subscriber of its own, and
/// `set_global_default` only succeeds once.
pub fn init_logging() -> anyhow::Result<()> {
    tracing_log::LogTracer::init()?;
    let (log_env_filter, _) = tracing_subscriber::reload::Layer::new(
        tracing_subscriber::filter::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::filter::EnvFilter::new("info")),
    );
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_span_events(
            tracing_subscriber::fmt::format::FmtSpan::NEW
                | tracing_subscriber::fmt::format::FmtSpan::CLOSE,
        )
        .compact();
    let subscriber = tracing_subscriber::Registry::default()
        .with(log_env_filter)
        .with(fmt_layer);
    tracing::subscriber::set_global_default(subscriber)?;
    Ok(())
}

/// The whole HTTP stack, shared by [`Server::run`] and the tests so that what is exercised is what
/// is served.
macro_rules! scim_app {
    ($routes:expr, $metrics_enabled:expr, $prometheus:expr) => {
        actix_web::App::new()
            .wrap(actix_web::middleware::NormalizePath::trim())
            .wrap(actix_web::middleware::Condition::new(
                $metrics_enabled,
                $prometheus,
            ))
            .wrap(tracing_actix_web::TracingLogger::default())
            .configure($routes)
            .route(
                "/health",
                actix_web::web::get().to(|| async { actix_web::HttpResponse::Ok().body("OK") }),
            )
            .default_service(actix_web::web::to(|| async {
                actix_web::HttpResponse::NotFound().finish()
            }))
    };
}

/// The routes, whether metrics are on, and the middleware recording them - built in one place so
/// that the tests stand up the same stack [`Server::run`] serves.
fn http_stack(
    app: &App,
) -> anyhow::Result<(
    impl Fn(&mut actix_web::web::ServiceConfig) + Clone + use<>,
    bool,
    actix_web_prom::PrometheusMetrics,
)> {
    let registry = app.metrics_registry();
    let enabled = registry.is_some();
    // With metrics off the middleware is built anyway, because `Condition` needs a value to wrap -
    // but against a throwaway registry, so that collectors nothing ever updates do not land in one
    // the consumer owns and serves.
    let prometheus =
        metrics::http_middleware(registry.cloned().unwrap_or_else(MetricsRegistry::new))?;
    Ok((app.configure(), enabled, prometheus))
}

/// A [`scimd::App`] plus the process around it: one place to configure SCIM, one to configure the
/// process.
pub struct Server {
    app: App,
    workers: usize,
    bind: Vec<String>,
    unix_bind: Vec<String>,
}

impl Server {
    #[must_use]
    pub fn new(app: App) -> Self {
        Self {
            app,
            workers: 2,
            bind: vec![],
            unix_bind: vec![],
        }
    }

    #[must_use]
    pub fn worker(mut self, workers: usize) -> Self {
        self.workers = workers;
        self
    }

    #[must_use]
    pub fn add_bind(mut self, bind: String) -> Self {
        self.bind.push(bind);
        self
    }

    #[must_use]
    pub fn add_unix_bind(mut self, bind: String) -> Self {
        self.unix_bind.push(bind);
        self
    }

    /// Takes the binds and worker count from the command line - see [`parse_args`].
    #[must_use]
    pub fn args(mut self, args: Args) -> Self {
        self.bind = args.bind;
        self.unix_bind = args.unix_bind;
        self.workers = args.workers;
        self
    }

    /// The default is conditional on there being *no* socket of either kind, not just no TCP one:
    /// a server told only about a Unix socket must not quietly also listen on TCP.
    fn tcp_binds(&self) -> Vec<&str> {
        if self.bind.is_empty() && self.unix_bind.is_empty() {
            vec![DEFAULT_BIND]
        } else {
            self.bind.iter().map(String::as_str).collect()
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let (routes, enabled, prometheus) = http_stack(&self.app)?;

        if enabled {
            tracing::info!("Serving Prometheus metrics from {}", metrics::ENDPOINT);
        }

        let mut server =
            HttpServer::new(move || scim_app!(routes.clone(), enabled, prometheus.clone()))
                .workers(self.workers);

        for bind in self.tcp_binds() {
            tracing::info!("Listening on TCP socket bound to {}", bind);
            server = server.bind(bind)?;
        }
        for bind in &self.unix_bind {
            tracing::info!("Listening on Unix socket bound to {}", bind);
            server = server.bind_uds(bind)?;
        }

        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;

        let _ = sd_notify::notify(&[sd_notify::NotifyState::Ready]);
        let server = server.disable_signals().run();
        tokio::pin!(server);
        tokio::select! {
            r = sigint.recv() => {
                tracing::info!("Received interrupt signal - shutting down gracefully");
                r.unwrap();
                let _ = sd_notify::notify(&[sd_notify::NotifyState::Stopping]);
                let ((), r) = tokio::join!(server.handle().stop(true), server);
                r.unwrap();
            },
            r = sigterm.recv() => {
                tracing::info!("Received term signal - shutting down gracefully");
                r.unwrap();
                let _ = sd_notify::notify(&[sd_notify::NotifyState::Stopping]);
                let ((), r) = tokio::join!(server.handle().stop(true), server);
                r.unwrap();
            },
            r = &mut server => {
                r.unwrap();
            }
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests;
