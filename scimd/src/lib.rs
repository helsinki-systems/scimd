use std::sync::Arc;

use actix_web::{HttpServer, web};
use tracing_actix_web::TracingLogger;
use tracing_subscriber::layer::SubscriberExt as _;

pub mod middleware;
pub mod models;

pub use actix_web::HttpResponse;
pub use async_trait::async_trait;
use uuid::Uuid;

#[async_trait]
pub trait Service {
    async fn create_user(
        &self,
        _user: models::User,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn replace_user(
        &self,
        _id: Uuid,
        _user: models::User,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn patch_user(
        &self,
        _id: Uuid,
        _op: models::PatchOp,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn delete_user(&self, _id: Uuid) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn create_group(
        &self,
        _group: models::Group,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn replace_group(
        &self,
        _id: Uuid,
        _group: models::Group,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn patch_group(
        &self,
        _id: Uuid,
        _op: models::PatchOp,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }

    async fn delete_group(&self, _id: Uuid) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }
}

pub struct App {
    service: Arc<dyn Service + Send + Sync>,
    workers: usize,
    bind: String,
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
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.create_user(user.into_inner()).await
}

async fn replace_user(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    user: web::Json<models::User>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.replace_user(*id, user.into_inner()).await
}

async fn patch_user(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    user: web::Json<models::PatchOp>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.patch_user(*id, user.into_inner()).await
}

async fn delete_user(
    state: web::Data<State>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.delete_user(*id).await
}

async fn create_group(
    state: web::Data<State>,
    group: web::Json<models::Group>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.create_group(group.into_inner()).await
}

async fn replace_group(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    group: web::Json<models::Group>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.replace_group(*id, group.into_inner()).await
}

async fn patch_group(
    state: web::Data<State>,
    id: web::Path<Uuid>,
    group: web::Json<models::PatchOp>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.patch_group(*id, group.into_inner()).await
}

async fn delete_group(
    state: web::Data<State>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
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
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
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

        tracing::info!("listening on {}", self.bind);
        HttpServer::new(move || {
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
        .workers(self.workers)
        .bind(&self.bind)?
        .run()
        .await?;
        Ok(())
    }
}

pub struct AppBuilder {
    service: Arc<dyn Service + Send + Sync>,
    workers: usize,
    bind: String,
    bearer_token: Option<String>,
}

impl AppBuilder {
    pub fn new(service: Arc<dyn Service + Send + Sync>) -> Self {
        Self {
            service,
            workers: 2,
            bind: "127.0.0.1:8080".to_owned(),
            bearer_token: None,
        }
    }

    pub fn worker(mut self, workers: usize) -> Self {
        self.workers = workers;
        self
    }

    pub fn bind(mut self, bind: String) -> Self {
        self.bind = bind;
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
            bind: self.bind,
            bearer_token: self.bearer_token,
        }
    }
}

pub fn run() {
    println!("Hello, world!");
}
