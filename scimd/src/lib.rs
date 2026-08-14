use std::collections::BTreeMap;
use std::sync::Arc;

use actix_web::{HttpServer, http, web};
use tracing_subscriber::layer::SubscriberExt as _;

mod bulk;
mod error;
pub(crate) mod metrics;
pub mod middleware;
pub mod models;
mod opts;
mod resource;

pub use actix_web::HttpResponse;
pub use async_trait::async_trait;
pub use error::{Error, Result};
pub use opts::{load_config, parse_args};
/// Re-exported so a consumer can hand scimd a registry it already owns without taking a
/// `prometheus` dependency of its own.
pub use prometheus::Registry as MetricsRegistry;
pub use resource::{Resource, ResourceService};

use metrics::{Metrics, Operation, Source};
use resource::ErasedHandler;

pub(crate) const SCIM_BASE_PATH: &str = "/v2";

/// Keyed by [`Resource::ENDPOINT`], and ordered so that `/ResourceTypes` and `/Schemas` render
/// deterministically.
type Registry = BTreeMap<&'static str, Arc<dyn ErasedHandler>>;

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

pub struct App {
    handlers: Arc<Registry>,
    workers: usize,
    bind: Vec<String>,
    unix_bind: Vec<String>,
    bearer_token: Option<String>,

    enable_patch: bool,
    filter_config: FilterConfig,
    bulk_config: BulkConfig,
    enable_sort: bool,
    metrics_config: MetricsConfig,
}

#[derive(Default)]
struct MetricsConfig {
    enabled: bool,
    registry: Option<MetricsRegistry>,
}

#[derive(Clone, Copy, Default)]
pub struct FilterConfig {
    enabled: bool,
    max_results: Option<u32>,
}

impl FilterConfig {
    pub fn new(enabled: bool, max_results: Option<u32>) -> Self {
        Self {
            enabled,
            max_results,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct BulkConfig {
    enabled: bool,
    max_operations: Option<u32>,
    max_payload_size: Option<u32>,
}

impl BulkConfig {
    pub fn new(enabled: bool, max_operations: Option<u32>, max_payload_size: Option<u32>) -> Self {
        Self {
            enabled,
            max_operations,
            max_payload_size,
        }
    }
}

#[derive(Clone)]
pub struct State {
    handlers: Arc<Registry>,
    bearer_token: Option<String>,

    enable_patch: bool,
    filter_settings: FilterConfig,
    bulk_settings: BulkConfig,
    enable_sort: bool,
    metrics: Option<Arc<Metrics>>,
}

impl State {
    /// Looks up the handler a route was registered for. Routes are only ever built from the
    /// registry, so a miss means the two went out of sync - a bug, not a bad request.
    fn handler(&self, endpoint: &str) -> Result<Arc<dyn ErasedHandler>> {
        self.handlers.get(endpoint).cloned().ok_or_else(|| {
            Error::InternalError(anyhow::anyhow!(
                "no handler registered for endpoint {endpoint}"
            ))
        })
    }

    /// Times a single resource operation and records how it went. Both the routes and the `/Bulk`
    /// executor go through here, which is the whole reason the SCIM layer is instrumented
    /// separately from the HTTP one.
    pub(crate) async fn observe<T>(
        &self,
        resource_type: &str,
        operation: Operation,
        source: Source,
        call: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        metrics::observe(
            self.metrics.as_deref(),
            resource_type,
            operation,
            source,
            call,
        )
        .await
    }

    pub(crate) fn observe_bulk_size(&self, operations: usize) {
        if let Some(metrics) = &self.metrics {
            metrics.observe_bulk_size(operations);
        }
    }
}

fn construct_response(body: impl serde::Serialize) -> Result<HttpResponse> {
    Ok(HttpResponse::Ok()
        .append_header((http::header::CONTENT_TYPE, "application/scim+json"))
        .json(body))
}

/// RFC 7644 section 3.3: a successful create answers `201` and points at the new resource. The
/// `Location` comes from the `meta.location` [`resource::finalize`] already filled in.
fn created_response(body: serde_json::Value) -> Result<HttpResponse> {
    let mut response = HttpResponse::Created();
    response.append_header((http::header::CONTENT_TYPE, "application/scim+json"));
    if let Some(location) = body
        .get("meta")
        .and_then(|m| m.get("location"))
        .and_then(serde_json::Value::as_str)
    {
        response.append_header((http::header::LOCATION, location));
    }
    Ok(response.json(body))
}

async fn list(
    state: web::Data<State>,
    endpoint: &'static str,
    query: web::Query<models::FilterQuery>,
) -> Result<HttpResponse> {
    let handler = state.handler(endpoint)?;
    construct_response(
        state
            .observe(
                handler.resource_type(),
                Operation::List,
                Source::Http,
                handler.list(query.into_inner()),
            )
            .await?,
    )
}

async fn create(
    state: web::Data<State>,
    endpoint: &'static str,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse> {
    let handler = state.handler(endpoint)?;
    created_response(
        state
            .observe(
                handler.resource_type(),
                Operation::Create,
                Source::Http,
                handler.create(body.into_inner()),
            )
            .await?,
    )
}

async fn search(
    state: web::Data<State>,
    endpoint: &'static str,
    request: web::Json<models::SearchRequest>,
) -> Result<HttpResponse> {
    let handler = state.handler(endpoint)?;
    construct_response(
        state
            .observe(
                handler.resource_type(),
                Operation::Search,
                Source::Http,
                handler.list(request.into_inner().into()),
            )
            .await?,
    )
}

/// `POST /.search` at the root queries every registered resource type at once.
///
/// The result is the concatenation of each handler's answer, in registry order, with
/// `totalResults` summed. `startIndex` and `count` reach each backend individually rather than
/// being applied across the whole, so paging a root search is approximate - page a single
/// resource type's `.search` when it has to be exact.
async fn search_all(
    state: web::Data<State>,
    request: web::Json<models::SearchRequest>,
) -> Result<HttpResponse> {
    let query: models::FilterQuery = request.into_inner().into();

    let mut total = 0;
    let mut resources = vec![];
    for handler in state.handlers.values() {
        let answer = state
            .observe(
                handler.resource_type(),
                Operation::Search,
                Source::Http,
                handler.list(clone_query(&query)),
            )
            .await;
        // A resource type that does not support listing is simply not part of the answer; one
        // unimplemented `list` must not take the whole root search down with it.
        let mut list = match answer {
            Ok(list) => list,
            Err(Error::MethodNotImplemented) => continue,
            Err(e) => return Err(e),
        };
        total += list
            .get("totalResults")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize;
        if let Some(found) = list.get_mut("Resources").and_then(|r| r.as_array_mut()) {
            resources.append(found);
        }
    }

    let mut combined = models::ListResponse::new(resources);
    combined.total_results = total;
    construct_response(combined)
}

/// [`models::FilterQuery`] is deliberately not `Clone` - it is a request-scoped value everywhere
/// else - so the fan-out copies it explicitly.
fn clone_query(query: &models::FilterQuery) -> models::FilterQuery {
    models::FilterQuery {
        filter: query.filter.clone(),
        sort_by: query.sort_by.clone(),
        sort_order: query.sort_order.clone(),
        attributes: query.attributes.clone(),
        excluded_attributes: query.excluded_attributes.clone(),
        start_index: query.start_index,
        count: query.count,
    }
}

/// `POST /Bulk` (RFC 7644 section 3.7). The body is taken as bytes rather than parsed JSON so that
/// `maxPayloadSize` is checked against what was actually received.
async fn bulk_operations(state: web::Data<State>, body: web::Bytes) -> Result<HttpResponse> {
    if !state.bulk_settings.enabled {
        return Err(Error::MethodNotImplemented);
    }
    if let Some(max) = state.bulk_settings.max_payload_size
        && body.len() > max as usize
    {
        return Err(Error::PayloadTooLarge(format!(
            "the request body is {} bytes, more than the {max} supported",
            body.len()
        )));
    }

    let request: models::BulkRequest = serde_json::from_slice(&body)
        .map_err(|e| Error::BadRequest(format!("invalid BulkRequest: {e}")))?;
    construct_response(bulk::execute(&state, request).await?)
}

async fn get(
    state: web::Data<State>,
    endpoint: &'static str,
    id: web::Path<String>,
) -> Result<HttpResponse> {
    let handler = state.handler(endpoint)?;
    construct_response(
        state
            .observe(
                handler.resource_type(),
                Operation::Get,
                Source::Http,
                handler.get(&id),
            )
            .await?,
    )
}

async fn replace(
    state: web::Data<State>,
    endpoint: &'static str,
    id: web::Path<String>,
    body: web::Json<serde_json::Value>,
) -> Result<HttpResponse> {
    let handler = state.handler(endpoint)?;
    construct_response(
        state
            .observe(
                handler.resource_type(),
                Operation::Replace,
                Source::Http,
                handler.replace(&id, body.into_inner()),
            )
            .await?,
    )
}

async fn patch(
    state: web::Data<State>,
    endpoint: &'static str,
    id: web::Path<String>,
    op: web::Json<models::PatchOp>,
) -> Result<HttpResponse> {
    let handler = state.handler(endpoint)?;
    let patched = state
        .observe(
            handler.resource_type(),
            Operation::Patch,
            Source::Http,
            handler.patch(&id, op.into_inner()),
        )
        .await?;
    match patched {
        Some(v) => construct_response(v),
        None => Ok(HttpResponse::NoContent().finish()),
    }
}

async fn delete(
    state: web::Data<State>,
    endpoint: &'static str,
    id: web::Path<String>,
) -> Result<HttpResponse> {
    let handler = state.handler(endpoint)?;
    state
        .observe(
            handler.resource_type(),
            Operation::Delete,
            Source::Http,
            handler.delete(&id),
        )
        .await?;
    Ok(HttpResponse::NoContent().finish())
}

fn resource_type_of(handler: &Arc<dyn ErasedHandler>) -> models::ResourceType {
    models::ResourceType::new(
        handler.resource_type(),
        handler.endpoint(),
        handler.schema_urn(),
    )
    .with_extensions(handler.extensions().into_iter().map(Into::into).collect())
}

/// Every schema the server knows about: each resource type's own, followed by its extensions.
/// Two resource types sharing an extension advertise it once.
fn schemas_of(state: &State) -> Vec<models::Schema> {
    let mut seen = std::collections::BTreeSet::new();
    let mut schemas = vec![];
    for handler in state.handlers.values() {
        let extensions = handler.extensions().into_iter().map(|e| e.schema);
        for schema in std::iter::once(handler.schema()).chain(extensions) {
            if seen.insert(schema.id.clone()) {
                schemas.push(schema);
            }
        }
    }
    schemas
}

async fn list_resource_types(state: web::Data<State>) -> Result<HttpResponse> {
    construct_response(models::ListResponse::new(
        state.handlers.values().map(resource_type_of).collect(),
    ))
}

async fn get_resource_type(state: web::Data<State>, id: web::Path<String>) -> Result<HttpResponse> {
    let id = id.into_inner();
    let handler = state
        .handlers
        .values()
        .find(|h| h.resource_type() == id)
        .ok_or(Error::NotFound(id))?;
    construct_response(resource_type_of(handler))
}

async fn list_schemas(state: web::Data<State>) -> Result<HttpResponse> {
    construct_response(models::ListResponse::new(schemas_of(&state)))
}

async fn get_schema(state: web::Data<State>, urn: web::Path<String>) -> Result<HttpResponse> {
    let urn = urn.into_inner();
    let schema = schemas_of(&state)
        .into_iter()
        .find(|s| s.id == urn)
        .ok_or(Error::NotFound(urn))?;
    construct_response(schema)
}

async fn service_provider(state: web::Data<State>) -> Result<HttpResponse> {
    let mut config = models::ServiceProviderConfig::default()
        .with_patch(state.enable_patch)
        .with_filter(
            state.filter_settings.enabled,
            state.filter_settings.max_results,
        )
        .with_bulk(
            state.bulk_settings.enabled,
            state.bulk_settings.max_operations,
            state.bulk_settings.max_payload_size,
        )
        .with_sort(state.enable_sort);
    if state.bearer_token.is_some() {
        config = config.add_authentication_scheme(
            models::service_provider_config::AuthenticationScheme::bearer(),
        );
    }

    construct_response(config)
}

async fn health_check() -> HttpResponse {
    HttpResponse::Ok().body("OK")
}

#[allow(clippy::unused_async)]
async fn not_found() -> actix_web::HttpResponse {
    actix_web::HttpResponse::NotFound().finish()
}

/// actix's own payload limit would otherwise reject a large bulk body before
/// [`bulk_operations`] can turn it into a SCIM `413`.
fn bulk_payload_config(state: &State) -> web::PayloadConfig {
    match state.bulk_settings.max_payload_size {
        Some(max) => web::PayloadConfig::new(max as usize + 1),
        None => web::PayloadConfig::default(),
    }
}

/// The whole HTTP stack, shared by [`App::run`] and the tests so that what is exercised is what is
/// served.
macro_rules! scim_app {
    ($state:expr, $metrics_enabled:expr, $prometheus:expr) => {
        actix_web::App::new()
            .wrap(actix_web::middleware::NormalizePath::trim())
            .wrap(actix_web::middleware::Condition::new(
                $metrics_enabled,
                $prometheus,
            ))
            .wrap(tracing_actix_web::TracingLogger::default())
            .configure({
                let state = $state;
                move |cfg| crate::configure(cfg, state)
            })
            .route("/health", actix_web::web::get().to(crate::health_check))
            .default_service(actix_web::web::to(crate::not_found))
    };
}

/// Builds the SCIM scope. Shared by [`App::run`] and the tests, so both exercise the same routing.
///
/// One actix resource is registered per endpoint rather than a single `/{resourceType}` catch-all;
/// that keeps actix's 404-versus-405 handling intact and stops the catch-all from swallowing the
/// discovery endpoints.
fn configure(cfg: &mut web::ServiceConfig, state: State) {
    let mut scope = web::scope(SCIM_BASE_PATH)
        .wrap(middleware::scim())
        .service(web::resource("/ServiceProviderConfig").route(web::get().to(service_provider)))
        .service(web::resource("/ResourceTypes").route(web::get().to(list_resource_types)))
        .service(web::resource("/ResourceTypes/{id}").route(web::get().to(get_resource_type)))
        .service(web::resource("/Schemas").route(web::get().to(list_schemas)))
        .service(web::resource("/Schemas/{urn}").route(web::get().to(get_schema)))
        .service(web::resource("/.search").route(web::post().to(search_all)))
        .service(
            web::resource("/Bulk")
                // So that an oversized body is rejected as a SCIM 413 rather than by actix.
                .app_data(bulk_payload_config(&state))
                .route(web::post().to(bulk_operations)),
        );

    for &endpoint in state.handlers.keys() {
        tracing::debug!("Registering SCIM routes for {SCIM_BASE_PATH}{endpoint}");
        scope = scope
            .service(
                web::resource(format!("{endpoint}/.search")).route(web::post().to(
                    move |state: web::Data<State>, request: web::Json<models::SearchRequest>| {
                        search(state, endpoint, request)
                    },
                )),
            )
            .service(
                web::resource(endpoint)
                    .route(web::get().to(
                        move |state: web::Data<State>, query: web::Query<models::FilterQuery>| {
                            list(state, endpoint, query)
                        },
                    ))
                    .route(web::post().to(
                        move |state: web::Data<State>, body: web::Json<serde_json::Value>| {
                            create(state, endpoint, body)
                        },
                    )),
            )
            .service(
                web::resource(format!("{endpoint}/{{id}}"))
                    .route(
                        web::get().to(move |state: web::Data<State>, id: web::Path<String>| {
                            get(state, endpoint, id)
                        }),
                    )
                    .route(web::put().to(
                        move |state: web::Data<State>,
                              id: web::Path<String>,
                              body: web::Json<serde_json::Value>| {
                            replace(state, endpoint, id, body)
                        },
                    ))
                    .route(web::patch().to(
                        move |state: web::Data<State>,
                              id: web::Path<String>,
                              op: web::Json<models::PatchOp>| {
                            patch(state, endpoint, id, op)
                        },
                    ))
                    .route(web::delete().to(
                        move |state: web::Data<State>, id: web::Path<String>| {
                            delete(state, endpoint, id)
                        },
                    )),
            );
    }

    cfg.app_data(web::Data::new(state)).service(scope);
}

impl App {
    fn metrics(&self) -> anyhow::Result<Arc<Metrics>> {
        let metrics = Metrics::new(self.metrics_config.registry.clone())?;
        metrics.set_resource_types(self.handlers.len());
        Ok(Arc::new(metrics))
    }

    fn state(&self, metrics: Option<Arc<Metrics>>) -> State {
        State {
            handlers: self.handlers.clone(),
            bearer_token: self.bearer_token.clone(),
            enable_patch: self.enable_patch,
            filter_settings: self.filter_config,
            bulk_settings: self.bulk_config,
            enable_sort: self.enable_sort,
            metrics,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let enabled = self.metrics_config.enabled;
        let metrics = self.metrics()?;

        let state = self.state(enabled.then(|| metrics.clone()));
        let prometheus = metrics.middleware();

        if self.bearer_token.is_none() {
            tracing::warn!("scimd runs without authorization. Please pass in a token!");
        }
        if self.handlers.is_empty() {
            tracing::warn!("scimd runs without any registered resource. Did you call register()?");
        }
        if enabled {
            tracing::info!("Serving Prometheus metrics from {}", metrics::ENDPOINT);
        }

        let mut server =
            HttpServer::new(move || scim_app!(state.clone(), enabled, prometheus.clone()))
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

pub struct AppBuilder {
    handlers: Registry,
    workers: usize,
    bind: Vec<String>,
    unix_bind: Vec<String>,
    bearer_token: Option<String>,

    enable_patch: bool,
    filter_settings: FilterConfig,
    bulk_settings: BulkConfig,
    enable_sort: bool,
    metrics_settings: MetricsConfig,
}

impl Default for AppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AppBuilder {
    pub fn new() -> Self {
        Self {
            handlers: Registry::new(),
            workers: 2,
            bind: vec![],
            unix_bind: vec![],
            bearer_token: None,
            enable_patch: false,
            filter_settings: FilterConfig::default(),
            bulk_settings: BulkConfig::default(),
            enable_sort: false,
            metrics_settings: MetricsConfig::default(),
        }
    }

    /// Registers the service backing a single SCIM resource type. `R` is normally inferred from the
    /// turbofish - `register::<User, _>(svc)` - and everything the framework needs to route and
    /// describe the resource comes from `R`'s [`Resource`] impl.
    pub fn register<R, S>(mut self, service: S) -> Self
    where
        R: Resource,
        S: ResourceService<R> + 'static,
    {
        let handler = Arc::new(resource::TypedHandler::new(service));
        if self.handlers.insert(R::ENDPOINT, handler).is_some() {
            tracing::warn!(
                "endpoint {} was registered more than once - the last registration wins",
                R::ENDPOINT
            );
        }
        self
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

    pub fn with_patch(mut self, supported: bool) -> Self {
        self.enable_patch = supported;
        self
    }

    pub fn with_filter(mut self, supported: bool, max_results: Option<u32>) -> Self {
        self.filter_settings = FilterConfig::new(supported, max_results);
        self
    }

    /// Enables `POST /Bulk`. Until this is called the endpoint answers `501`
    pub fn with_bulk(
        mut self,
        supported: bool,
        max_operations: Option<u32>,
        max_payload_size: Option<u32>,
    ) -> Self {
        self.bulk_settings = BulkConfig::new(supported, max_operations, max_payload_size);
        self
    }

    pub fn with_sort(mut self, supported: bool) -> Self {
        self.enable_sort = supported;
        self
    }

    /// Enables `GET /metrics`. Until this is called nothing is recorded and the path 404s.
    ///
    /// The endpoint is served outside the SCIM scope, so it answers a Prometheus scraper without a
    /// bearer token
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.metrics_settings.enabled = enabled;
        self
    }

    /// Records into a registry the consumer owns, so that a service's own metrics and scimd's are
    /// served from the same endpoint. Implies [`Self::with_metrics`].
    pub fn metrics_registry(mut self, registry: MetricsRegistry) -> Self {
        self.metrics_settings = MetricsConfig {
            enabled: true,
            registry: Some(registry),
        };
        self
    }

    pub fn build(self) -> App {
        App {
            handlers: Arc::new(self.handlers),
            workers: self.workers,
            bind: if self.bind.is_empty() && self.unix_bind.is_empty() {
                vec!["127.0.0.1:8080".into()]
            } else {
                self.bind
            },
            unix_bind: self.unix_bind,
            bearer_token: self.bearer_token,

            enable_patch: self.enable_patch,
            filter_config: self.filter_settings,
            bulk_config: self.bulk_settings,
            enable_sort: self.enable_sort,
            metrics_config: self.metrics_settings,
        }
    }
}

#[cfg(test)]
mod tests;
