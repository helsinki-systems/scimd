use std::sync::Arc;

use actix_web::{web, HttpServer};

pub mod models;

pub use actix_web::HttpResponse;
pub use async_trait::async_trait;
pub use log::{error, info, warn};
pub use uuid::Uuid;

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

    async fn delete_group(&self, _id: Uuid) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        Ok(HttpResponse::MethodNotAllowed().finish())
    }
}

pub struct App {
    service: Arc<dyn Service + Send + Sync>,
    workers: usize,
    bind: String,
}

#[derive(Clone)]
pub struct State {
    service: Arc<dyn Service + Send + Sync>,
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

async fn delete_group(
    state: web::Data<State>,
    id: web::Path<Uuid>,
) -> Result<HttpResponse, Box<dyn std::error::Error>> {
    state.service.delete_group(*id).await
}

impl App {
    pub async fn run(&self) -> std::io::Result<()> {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

        let state = State {
            service: self.service.clone(),
        };

        log::info!("listening on {}", self.bind);
        HttpServer::new(move || {
            actix_web::App::new()
                .app_data(actix_web::web::Data::new(state.clone()))
                .wrap(actix_web::middleware::Logger::default())
                .wrap(actix_web::middleware::NormalizePath::trim())
                .route("/v2/Users", web::post().to(create_user))
                .route("/v2/Users/{id}", web::put().to(replace_user))
                .route("/v2/Users/{id}", web::delete().to(delete_user))
                .route("/v2/Groups", web::post().to(create_group))
                .route("/v2/Groups/{id}", web::put().to(replace_group))
                .route("/v2/Groups/{id}", web::delete().to(delete_group))
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
}

impl AppBuilder {
    pub fn new(service: Arc<dyn Service + Send + Sync>) -> Self {
        Self {
            service,
            workers: 2,
            bind: "127.0.0.1:8080".to_owned(),
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

    pub fn build(self) -> App {
        App {
            service: self.service,
            workers: self.workers,
            bind: self.bind,
        }
    }
}

pub fn run() {
    println!("Hello, world!");
}
