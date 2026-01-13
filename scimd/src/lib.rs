use std::sync::Arc;

use actix_web::{HttpServer, web};
use tracing_actix_web::TracingLogger;
use tracing_subscriber::layer::SubscriberExt as _;
use uuid::Uuid;

mod error;
pub mod middleware;
pub mod models;
mod opts;

pub use actix_web::HttpResponse;
pub use async_trait::async_trait;
pub use error::Result;
pub use opts::parse_args;

#[async_trait]
pub trait Service {
    async fn create_user(&self, _user: models::User) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn replace_user(&self, _id: Uuid, _user: models::User) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn patch_user(&self, _id: Uuid, _op: models::PatchOp) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn delete_user(&self, _id: Uuid) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn create_group(&self, _group: models::Group) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn replace_group(&self, _id: Uuid, _group: models::Group) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn patch_group(&self, _id: Uuid, _op: models::PatchOp) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn delete_group(&self, _id: Uuid) -> Result<HttpResponse> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }
}

pub struct App {
    service: Arc<dyn Service + Send + Sync>,
    workers: usize,
    bind: Vec<String>,
    unix_bind: Vec<String>,
    bearer_token: Option<String>,
}

#[derive(Clone)]
pub struct State {
    service: Arc<dyn Service + Send + Sync>,
    bearer_token: Option<String>,
}

async fn create_user(
    state: web::Data<State>,
    user: web::Json<models::User>,
) -> Result<HttpResponse> {
    state.service.create_user(user.into_inner()).await
}

async fn replace_user(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    user: web::Json<models::User>,
) -> Result<HttpResponse> {
    state.service.replace_user(*id, user.into_inner()).await
}

async fn patch_user(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    user: web::Json<models::PatchOp>,
) -> Result<HttpResponse> {
    state.service.patch_user(*id, user.into_inner()).await
}

async fn delete_user(state: web::Data<State>, id: web::Path<Uuid>) -> Result<HttpResponse> {
    state.service.delete_user(*id).await
}

async fn create_group(
    state: web::Data<State>,
    group: web::Json<models::Group>,
) -> Result<HttpResponse> {
    state.service.create_group(group.into_inner()).await
}

async fn replace_group(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    group: web::Json<models::Group>,
) -> Result<HttpResponse> {
    state.service.replace_group(*id, group.into_inner()).await
}

async fn patch_group(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    group: web::Json<models::PatchOp>,
) -> Result<HttpResponse> {
    state.service.patch_group(*id, group.into_inner()).await
}

async fn delete_group(state: web::Data<State>, id: web::Path<Uuid>) -> Result<HttpResponse> {
    state.service.delete_group(*id).await
}

async fn health_check() -> HttpResponse {
    HttpResponse::Ok().body("OK")
}

#[allow(clippy::unused_async)]
async fn not_found() -> actix_web::HttpResponse {
    actix_web::HttpResponse::NotFound().finish()
}

impl App {
    pub async fn run(&self) -> anyhow::Result<()> {
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

        let state = State {
            service: self.service.clone(),
            bearer_token: self.bearer_token.clone(),
        };
        if self.bearer_token.is_none() {
            tracing::warn!("scimd runs without authorization. Please pass in a token!");
        }

        let mut server = HttpServer::new(move || {
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(state.clone()))
                .wrap(actix_web::middleware::NormalizePath::trim())
                .wrap(TracingLogger::default())
                .service(
                    web::scope("/v2")
                        .wrap(middleware::token())
                        .route("/Users", web::post().to(create_user))
                        .route("/Users/{id}", web::put().to(replace_user))
                        .route("/Users/{id}", web::patch().to(patch_user))
                        .route("/Users/{id}", web::delete().to(delete_user))
                        .route("/Groups", web::post().to(create_group))
                        .route("/Groups/{id}", web::put().to(replace_group))
                        .route("/Groups/{id}", web::patch().to(patch_group))
                        .route("/Groups/{id}", web::delete().to(delete_group)),
                )
                .route("/health", web::get().to(health_check))
                .default_service(web::to(not_found))
        })
        .workers(self.workers);

        for bind in &self.bind {
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

        let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Ready]);
        let server = server.disable_signals().run();
        tokio::pin!(server);
        tokio::select! {
            r = sigint.recv() => {
                tracing::info!("Received interrupt signal - shutting down gracefully");
                r.unwrap();
                let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Stopping]);
                let ((), r) = tokio::join!(server.handle().stop(true), server);
                r.unwrap();
            },
            r = sigterm.recv() => {
                tracing::info!("Received term signal - shutting down gracefully");
                r.unwrap();
                let _ = sd_notify::notify(false, &[sd_notify::NotifyState::Stopping]);
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

pub struct AppBuilder {
    service: Arc<dyn Service + Send + Sync>,
    workers: usize,
    bind: Vec<String>,
    unix_bind: Vec<String>,
    bearer_token: Option<String>,
}

impl AppBuilder {
    pub fn new(service: Arc<dyn Service + Send + Sync>) -> Self {
        Self {
            service,
            workers: 2,
            bind: vec![],
            unix_bind: vec![],
            bearer_token: None,
        }
    }

    pub fn worker(mut self, workers: usize) -> Self {
        self.workers = workers;
        self
    }

    pub fn add_bind(mut self, bind: String) -> Self {
        self.bind.push(bind);
        self
    }

    pub fn add_unix_bind(mut self, bind: String) -> Self {
        self.unix_bind.push(bind);
        self
    }

    pub fn args(mut self, args: opts::Args) -> Self {
        self.bind = args.bind;
        self.unix_bind = args.unix_bind;
        self.workers = args.workers;
        self
    }

    pub fn bearer_token(mut self, bearer_token: Option<String>) -> Self {
        self.bearer_token = bearer_token;
        self
    }

    pub fn build(self) -> App {
        App {
            service: self.service,
            workers: self.workers,
            bind: if self.bind.is_empty() && self.unix_bind.is_empty() {
                vec!["127.0.0.1:8080".into()]
            } else {
                self.bind
            },
            unix_bind: self.unix_bind,
            bearer_token: self.bearer_token,
        }
    }
}
