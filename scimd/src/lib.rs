//! The actix-web adapter for [`scimd_core`].
//!
//! [`AppBuilder`] wraps a configured [`ScimBuilder`] and [`App::configure`] hands its routes to an
//! actix `App`, so a host that already runs its own `HttpServer` mounts SCIM into it and nothing
//! here knows about sockets, signals or a command line. The standalone daemon that does own those
//! things is the `scimd-server` crate.

use std::sync::Arc;

use actix_web::{HttpMessage as _, HttpRequest, http, web};

mod error;
pub mod middleware;

pub use actix_web::HttpResponse;
pub use async_trait::async_trait;
pub use error::{Error, Result, ScimError};
pub use prometheus::Registry as MetricsRegistry;
pub use scimd_core::filter;
pub use scimd_core::{
    Resource, ResourceService, SCIM_BASE_PATH, Scim, ScimBuilder, ScimConfig, ScimMetrics,
    StatusCode, models,
};

use middleware::SCIM_MEDIA_TYPE;

type HttpResult = std::result::Result<HttpResponse, ScimError>;

/// The request context `C` a [`ResourceService`] takes. The host's own middleware puts it into the
/// request extensions, and scimd reads it from there.
///
/// `C` is cloned once per request, so it should be cheap to clone, such as a `Copy` id or an `Arc`.
///
/// ```
/// #[derive(Clone)]
/// struct Tenant(u32);
/// impl scimd::RequestContext for Tenant {}
/// ```
pub trait RequestContext: Clone + Sync + 'static {
    /// `None` answers a `500`. A request that reaches the mount without its context is a host bug.
    fn from_request(req: &HttpRequest) -> Option<Self> {
        req.extensions().get::<Self>().cloned()
    }
}

/// No context at all, which every request has.
impl RequestContext for () {
    fn from_request(_: &HttpRequest) -> Option<Self> {
        Some(())
    }
}

/// [`RequestContext`] as an extractor. Not `web::ReqData`, because its missing-data error is plain
/// text and nobody inserts `()`.
struct Context<C>(C);

impl<C: RequestContext> actix_web::FromRequest for Context<C> {
    type Error = ScimError;
    type Future = std::future::Ready<std::result::Result<Self, ScimError>>;

    fn from_request(req: &HttpRequest, _: &mut actix_web::dev::Payload) -> Self::Future {
        std::future::ready(
            <C as RequestContext>::from_request(req)
                .map(Context)
                .ok_or_else(|| {
                    tracing::error!(
                        "the host did not provide a {} for {}. It has to put one into the \
                         request extensions before the request reaches scimd",
                        std::any::type_name::<C>(),
                        req.path()
                    );
                    Error::InternalError(anyhow::anyhow!("missing request context")).into()
                }),
        )
    }
}

/// How the mount authenticates. Owned and `Send`, unlike [`middleware::Auth`], because
/// [`App::configure`]'s closure is cloned into every worker.
#[derive(Clone, Debug)]
enum Authentication {
    Bearer(String),
    None,
    Upstream,
}

impl Authentication {
    fn as_middleware(&self) -> middleware::Auth {
        match self {
            Self::Bearer(token) => middleware::Auth::bearer(token),
            Self::None => middleware::Auth::None,
            Self::Upstream => middleware::Auth::Upstream,
        }
    }
}

pub struct App<C = ()> {
    scim: Scim<C>,
    auth: Authentication,
    /// The registry the SCIM collectors were registered into, or `None` when metrics are off.
    registry: Option<MetricsRegistry>,
}

fn construct_response(body: impl serde::Serialize) -> HttpResult {
    Ok(HttpResponse::Ok()
        .append_header((http::header::CONTENT_TYPE, SCIM_MEDIA_TYPE))
        .json(body))
}

/// RFC 7644 section 3.3: a successful create answers `201` and points at the new resource.
fn created_response(body: serde_json::Value) -> HttpResult {
    let mut response = HttpResponse::Created();
    response.append_header((http::header::CONTENT_TYPE, SCIM_MEDIA_TYPE));
    if let Some(location) = body
        .pointer("/meta/location")
        .and_then(serde_json::Value::as_str)
    {
        response.append_header((http::header::LOCATION, location));
    }
    Ok(response.json(body))
}

async fn list<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    endpoint: &'static str,
    query: web::Query<models::FilterQuery>,
) -> HttpResult {
    construct_response(scim.list(&ctx, endpoint, query.into_inner()).await?)
}

async fn create<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    endpoint: &'static str,
    body: web::Json<serde_json::Value>,
) -> HttpResult {
    created_response(scim.create(&ctx, endpoint, body.into_inner()).await?)
}

async fn search<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    endpoint: &'static str,
    request: web::Json<models::SearchRequest>,
) -> HttpResult {
    construct_response(
        scim.search(&ctx, endpoint, request.into_inner().into())
            .await?,
    )
}

async fn search_all<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    request: web::Json<models::SearchRequest>,
) -> HttpResult {
    construct_response(scim.search_all(&ctx, request.into_inner().into()).await?)
}

/// The body is taken as bytes rather than parsed JSON so that `maxPayloadSize` is checked against
/// what was actually received.
async fn bulk_operations<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    body: web::Bytes,
) -> HttpResult {
    construct_response(scim.bulk_bytes(&ctx, &body).await?)
}

async fn get<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    endpoint: &'static str,
    id: web::Path<String>,
) -> HttpResult {
    construct_response(scim.get(&ctx, endpoint, &id).await?)
}

async fn replace<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    endpoint: &'static str,
    id: web::Path<String>,
    body: web::Json<serde_json::Value>,
) -> HttpResult {
    construct_response(scim.replace(&ctx, endpoint, &id, body.into_inner()).await?)
}

async fn patch<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    endpoint: &'static str,
    id: web::Path<String>,
    op: web::Json<models::PatchOp>,
) -> HttpResult {
    match scim.patch(&ctx, endpoint, &id, op.into_inner()).await? {
        Some(v) => construct_response(v),
        None => Ok(HttpResponse::NoContent().finish()),
    }
}

async fn delete<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    Context(ctx): Context<C>,
    endpoint: &'static str,
    id: web::Path<String>,
) -> HttpResult {
    scim.delete(&ctx, endpoint, &id).await?;
    Ok(HttpResponse::NoContent().finish())
}

async fn list_resource_types<C: RequestContext>(scim: web::Data<Scim<C>>) -> HttpResult {
    construct_response(scim.resource_types())
}

async fn get_resource_type<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    id: web::Path<String>,
) -> HttpResult {
    construct_response(scim.resource_type(&id)?)
}

async fn list_schemas<C: RequestContext>(scim: web::Data<Scim<C>>) -> HttpResult {
    construct_response(scim.schemas())
}

async fn get_schema<C: RequestContext>(
    scim: web::Data<Scim<C>>,
    urn: web::Path<String>,
) -> HttpResult {
    construct_response(scim.schema(&urn)?)
}

async fn service_provider<C: RequestContext>(scim: web::Data<Scim<C>>) -> HttpResult {
    construct_response(scim.service_provider_config())
}

/// A body that is not JSON, or not the JSON a route expects, answers a SCIM error document rather
/// than actix's plain text. Any `Content-Type` is read as JSON, because some IdPs send none or
/// `text/plain`.
fn json_config() -> web::JsonConfig {
    web::JsonConfig::default()
        .content_type_required(false)
        .content_type(|_| true)
        .error_handler(|e, _| {
            let error = match e {
                actix_web::error::JsonPayloadError::OverflowKnownLength { .. }
                | actix_web::error::JsonPayloadError::Overflow { .. } => {
                    Error::PayloadTooLarge(e.to_string())
                }
                e => Error::BadRequest(format!("invalid JSON body: {e}")),
            };
            ScimError::from(error).into()
        })
}

/// A malformed query string, such as `?count=abc`, also answers a SCIM error document.
fn query_config() -> web::QueryConfig {
    web::QueryConfig::default().error_handler(|e, _| {
        ScimError::from(Error::BadRequest(format!("invalid query: {e}"))).into()
    })
}

/// Without this a mounted scope would fall through to the host's `default_service`, and a client
/// asking for a resource type that does not exist would get whatever that host serves - very
/// likely an HTML page - instead of a SCIM error document.
#[allow(clippy::unused_async)]
async fn scim_not_found(req: actix_web::HttpRequest) -> HttpResult {
    Err(Error::NotFound(req.path().to_owned()).into())
}

/// Binds `endpoint` into a handler, so that the route carries no borrow of the loop below.
macro_rules! bind {
    ($handler:ident, $endpoint:expr $(, $arg:ident : $ty:ty)*) => {
        move |scim: web::Data<Scim<C>>, ctx: Context<C> $(, $arg: $ty)*| {
            $handler(scim, ctx, $endpoint $(, $arg)*)
        }
    };
}

/// Builds the SCIM scope onto `cfg`.
///
/// One actix resource is registered per endpoint rather than a single `/{resourceType}` catch-all;
/// that keeps actix's 404-versus-405 handling intact and stops the catch-all from swallowing the
/// discovery endpoints.
fn configure<C: RequestContext>(
    cfg: &mut web::ServiceConfig,
    scim: Scim<C>,
    auth: middleware::Auth,
) {
    // Sized so that an oversized bulk body is rejected as a SCIM 413 rather than by actix's own
    // payload limit.
    let payload = match scim.config().bulk().max_payload_size {
        Some(max) => web::PayloadConfig::new(max as usize + 1),
        None => web::PayloadConfig::default(),
    };

    let base = scim.config().scope_path().to_owned();
    let mut scope = web::scope(&base)
        .wrap(middleware::scim(auth))
        .app_data(json_config())
        .app_data(query_config())
        .service(
            web::resource("/ServiceProviderConfig").route(web::get().to(service_provider::<C>)),
        )
        .service(web::resource("/ResourceTypes").route(web::get().to(list_resource_types::<C>)))
        .service(web::resource("/ResourceTypes/{id}").route(web::get().to(get_resource_type::<C>)))
        .service(web::resource("/Schemas").route(web::get().to(list_schemas::<C>)))
        .service(web::resource("/Schemas/{urn}").route(web::get().to(get_schema::<C>)))
        .service(web::resource("/.search").route(web::post().to(search_all::<C>)))
        .service(
            web::resource("/Bulk")
                .app_data(payload)
                .route(web::post().to(bulk_operations::<C>)),
        );

    for endpoint in scim.endpoints() {
        tracing::debug!("Registering SCIM routes for {base}{endpoint}");
        scope = scope
            .service(
                web::resource(format!("{endpoint}/.search")).route(web::post().to(bind!(
                    search,
                    endpoint,
                    request: web::Json<models::SearchRequest>
                ))),
            )
            .service(
                web::resource(endpoint)
                    .route(web::get().to(bind!(
                        list,
                        endpoint,
                        query: web::Query<models::FilterQuery>
                    )))
                    .route(web::post().to(bind!(
                        create,
                        endpoint,
                        body: web::Json<serde_json::Value>
                    ))),
            )
            .service(
                web::resource(format!("{endpoint}/{{id}}"))
                    .route(web::get().to(bind!(get, endpoint, id: web::Path<String>)))
                    .route(web::put().to(bind!(
                        replace,
                        endpoint,
                        id: web::Path<String>,
                        body: web::Json<serde_json::Value>
                    )))
                    .route(web::patch().to(bind!(
                        patch,
                        endpoint,
                        id: web::Path<String>,
                        op: web::Json<models::PatchOp>
                    )))
                    .route(web::delete().to(bind!(delete, endpoint, id: web::Path<String>))),
            );
    }

    // On the scope, not on `cfg`: a host calls `.configure()` on its own `App`, and app-level data
    // would put a `Scim` in their extractor namespace and let a second mount silently win.
    cfg.service(
        scope
            .app_data(web::Data::new(scim))
            .default_service(web::to(scim_not_found)),
    );
}

impl<C: RequestContext> App<C> {
    /// The SCIM routes, for a host that already runs its own `HttpServer`.
    ///
    /// Build it once and clone it into the per-worker factory; the returned value is cheap to
    /// clone and carries no borrow of `self`.
    ///
    /// ```no_run
    /// # fn f(app: scimd::App) {
    /// let scim_routes = app.configure();
    /// actix_web::HttpServer::new(move || {
    ///     actix_web::App::new().configure(scim_routes.clone())
    /// });
    /// # }
    /// ```
    ///
    /// Authentication and `Accept` negotiation are part of the mount and run before the scope's own
    /// 404, so an unauthenticated request to a path inside it is a `401`, not a `404`.
    pub fn configure(&self) -> impl Fn(&mut web::ServiceConfig) + Clone + use<C> {
        // Warned about here rather than in `run()` so that a host that mounts the routes hears it
        // too - the mount is where "authentication happens somewhere else" is easiest to assume.
        match &self.auth {
            Authentication::None => {
                tracing::warn!("scimd runs without authorization. Please pass in a token!");
            }
            // Deliberate, so said out loud but not warned about.
            Authentication::Upstream => {
                tracing::debug!("scimd leaves authentication to the host it is mounted in");
            }
            Authentication::Bearer(_) => {}
        }
        if self.scim.endpoints().is_empty() {
            tracing::warn!("scimd runs without any registered resource. Did you call register()?");
        }

        let scim = self.scim.clone();
        let auth = self.auth.clone();
        move |cfg: &mut web::ServiceConfig| {
            configure(cfg, scim.clone(), auth.as_middleware());
        }
    }

    /// The registry the SCIM collectors were registered into, for whoever serves the exposition.
    ///
    /// A host that mounts scimd gets the SCIM series - operations, durations, bulk sizes,
    /// registered resource types - but not the per-route HTTP ones, which come from actix-web-prom
    /// wrapped around a whole app and are the host's to add.
    #[must_use]
    pub fn metrics_registry(&self) -> Option<&MetricsRegistry> {
        self.registry.as_ref()
    }
}

/// Wraps a configured [`ScimBuilder`] with what only the HTTP layer decides: the bearer token and
/// where the metrics go.
pub struct AppBuilder<C = ()> {
    core: ScimBuilder<C>,
    bearer_token: Option<String>,
    upstream_authentication: bool,
    registry: Option<MetricsRegistry>,
    /// `None` until [`AppBuilder::with_metrics`] says either way, so that it and
    /// [`AppBuilder::metrics_registry`] compose in either order.
    metrics_enabled: Option<bool>,
}

// Not derived, because derive would demand `C: Default`.
impl<C> Default for AppBuilder<C> {
    fn default() -> Self {
        Self {
            core: ScimBuilder::default(),
            bearer_token: None,
            upstream_authentication: false,
            registry: None,
            metrics_enabled: None,
        }
    }
}

impl<C: RequestContext> AppBuilder<C> {
    #[must_use]
    pub fn new(core: ScimBuilder<C>) -> Self {
        Self {
            core,
            ..Self::default()
        }
    }

    /// The token every request to the SCIM scope must present, and the reason
    /// `/ServiceProviderConfig` advertises bearer authentication.
    ///
    /// `None` does not mean "unauthenticated": a request presenting a token anyway is refused. A
    /// host that authenticates upstream wants [`Self::authentication_handled_upstream`].
    ///
    /// The advertisement is added in [`Self::build`] rather than here, so that this and
    /// [`ScimBuilder::advertise_authentication_scheme`] compose in either order.
    #[must_use]
    pub fn bearer_token(mut self, bearer_token: Option<String>) -> Self {
        self.bearer_token = bearer_token;
        self
    }

    /// The host authenticates every request before it reaches the mount, so scimd does not read the
    /// `Authorization` header at all - not even to refuse a token it was not configured with.
    /// A [`Self::bearer_token`] wins over this, whichever order the two are called in.
    #[must_use]
    pub fn authentication_handled_upstream(mut self, upstream: bool) -> Self {
        self.upstream_authentication = upstream;
        self
    }

    /// Records the SCIM metrics into a registry of scimd's own - see [`App::metrics_registry`].
    /// Until this or [`Self::metrics_registry`] is called nothing is recorded. Resolved in
    /// [`Self::build`], so `with_metrics(false)` silences a registry passed before *or* after it.
    #[must_use]
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.metrics_enabled = Some(enabled);
        self
    }

    /// Records into a registry the consumer owns, so that a service's own metrics and scimd's are
    /// served from the same endpoint. Implies [`Self::with_metrics`] unless that said otherwise.
    #[must_use]
    pub fn metrics_registry(mut self, registry: MetricsRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    #[must_use]
    pub fn build(self) -> App<C> {
        let mut core = self.core;

        if self.bearer_token.is_some() {
            core = core.advertise_authentication_scheme(
                models::service_provider_config::AuthenticationScheme::bearer(),
            );
        }

        let auth = match (self.bearer_token, self.upstream_authentication) {
            (Some(token), upstream) => {
                if upstream {
                    tracing::warn!("scimd checks the token it was given, upstream or not");
                }
                Authentication::Bearer(token)
            }
            (None, true) => Authentication::Upstream,
            (None, false) => Authentication::None,
        };

        // A registry means metrics unless `with_metrics` said otherwise, whichever order the two
        // were called in.
        let enabled = self.metrics_enabled.unwrap_or(self.registry.is_some());
        let registry = enabled.then(|| self.registry.unwrap_or_default());

        // Registered once, here, so that `configure()` can be called as often as a host likes -
        // registering the same collectors into one registry twice is an error. A registry the
        // consumer owns may already hold another mount's collectors, and that has to cost metrics
        // rather than routes, so the failure is a warning.
        if let Some(registry) = &registry {
            match ScimMetrics::new(registry) {
                Ok(metrics) => core = core.metrics(Arc::new(metrics)),
                Err(e) => tracing::warn!("scimd runs without SCIM metrics: {e}"),
            }
        }

        App {
            scim: core.build(),
            auth,
            registry,
        }
    }
}

#[cfg(test)]
mod tests;
