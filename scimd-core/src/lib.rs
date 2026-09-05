//! A SCIM 2.0 service provider, without a web framework.
//!
//! [`Scim`] is the whole server bar the HTTP: a registry of resource types, the dispatch onto the
//! [`ResourceService`] backing each one, `/Bulk`, the discovery documents, and the SCIM-layer
//! metrics. Its methods take and return SCIM values, not requests and responses, so an adapter is
//! only routing plus serialisation - see the `scimd` crate for the actix-web one.
//!
//! ```no_run
//! # async fn f(svc: impl scimd_core::ResourceService<scimd_core::models::User> + 'static) -> scimd_core::Result<()> {
//! let scim = scimd_core::ScimBuilder::new()
//!     .register::<scimd_core::models::User, _>(svc)
//!     .build();
//! let users = scim.list("/Users", Default::default()).await?;
//! # Ok(()) }
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::Value;

mod bulk;
pub mod filter;
pub mod headers;
mod metrics;
mod resource;

pub use metrics::ScimMetrics;
pub use resource::ResourceService;
pub use scimd_types::{Error, Resource, Result, SCIM_BASE_PATH, StatusCode, models};

use models::service_provider_config::{AuthenticationScheme, BulkConfig, FilterConfig};
use models::{
    BulkRequest, BulkResponse, FilterQuery, ListResponse, PatchOp, ResourceType, Schema,
    ServiceProviderConfig,
};
use resource::ErasedHandler;

/// Keyed by [`Resource::ENDPOINT`], and ordered so that `/ResourceTypes` and `/Schemas` render
/// deterministically. The `&'static str` key is what lets a route closure an adapter builds from
/// [`Scim::endpoints`] be `'static`.
type Registry = BTreeMap<&'static str, Arc<dyn ErasedHandler>>;

/// What this server supports, as `/ServiceProviderConfig` advertises it and as the operations
/// enforce it.
#[derive(Debug, Clone)]
pub struct ScimConfig {
    scope_path: String,
    location_base: String,
    enable_patch: bool,
    filter: FilterConfig,
    bulk: BulkConfig,
    enable_sort: bool,
    authentication_schemes: Vec<AuthenticationScheme>,
}

impl Default for ScimConfig {
    fn default() -> Self {
        Self {
            scope_path: SCIM_BASE_PATH.to_owned(),
            location_base: SCIM_BASE_PATH.to_owned(),
            enable_patch: false,
            filter: FilterConfig::default(),
            bulk: BulkConfig::default(),
            enable_sort: false,
            authentication_schemes: vec![],
        }
    }
}

impl ScimConfig {
    /// Where an adapter should mount the SCIM routes, e.g. `/v2`.
    #[must_use]
    pub fn scope_path(&self) -> &str {
        &self.scope_path
    }

    /// So that an adapter can size its own body limit to match the `413` `/Bulk` will raise.
    #[must_use]
    pub fn bulk(&self) -> BulkConfig {
        self.bulk
    }
}

/// Normalises a configured path: a leading slash, no trailing one, and one slash between segments.
/// An empty path would make `web::scope("")` and a bare `meta.location` and is refused in favour of
/// the default.
fn normalise_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.trim().split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return None;
    }
    Some(format!("/{}", segments.join("/")))
}

#[derive(Clone)]
pub struct Scim {
    handlers: Arc<Registry>,
    config: Arc<ScimConfig>,
    metrics: Option<Arc<ScimMetrics>>,
}

impl Scim {
    #[must_use]
    pub fn config(&self) -> &ScimConfig {
        &self.config
    }

    /// Every registered endpoint, in the order `/ResourceTypes` renders them. An adapter builds one
    /// route per entry rather than a single catch-all, so that its own 404-versus-405 handling and
    /// the discovery endpoints both survive.
    #[must_use]
    pub fn endpoints(&self) -> Vec<&'static str> {
        self.handlers.keys().copied().collect()
    }

    fn base(&self) -> &str {
        &self.config.location_base
    }

    /// Routes are only ever built from [`Self::endpoints`], so a miss means the two went out of
    /// sync - a bug, not a bad request.
    fn handler(&self, endpoint: &str) -> Result<&Arc<dyn ErasedHandler>> {
        self.handlers.get(endpoint).ok_or_else(|| {
            Error::InternalError(anyhow::anyhow!(
                "no handler registered for endpoint {endpoint}"
            ))
        })
    }

    async fn observe<T>(
        &self,
        resource_type: &str,
        operation: metrics::Operation,
        source: metrics::Source,
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

    fn observe_bulk_size(&self, operations: usize) {
        if let Some(metrics) = &self.metrics {
            metrics.observe_bulk_size(operations);
        }
    }

    /// # Errors
    ///
    /// Whatever the [`ResourceService`] answers, or [`Error::MethodNotImplemented`] if it does not
    /// implement `list`.
    pub async fn list(&self, endpoint: &str, query: FilterQuery) -> Result<Value> {
        self.list_as(endpoint, query, metrics::Operation::List)
            .await
    }

    /// `POST {endpoint}/.search` - the same query as [`Self::list`], counted apart from it.
    pub async fn search(&self, endpoint: &str, query: FilterQuery) -> Result<Value> {
        self.list_as(endpoint, query, metrics::Operation::Search)
            .await
    }

    async fn list_as(
        &self,
        endpoint: &str,
        query: FilterQuery,
        operation: metrics::Operation,
    ) -> Result<Value> {
        let handler = self.handler(endpoint)?;
        self.observe(
            handler.resource_type(),
            operation,
            metrics::Source::Http,
            handler.list(self.base(), query),
        )
        .await
    }

    /// # Errors
    ///
    /// [`Error::BadRequest`] if `body` is not a valid resource of this type, else whatever the
    /// [`ResourceService`] answers.
    pub async fn create(&self, endpoint: &str, body: Value) -> Result<Value> {
        let handler = self.handler(endpoint)?;
        self.observe(
            handler.resource_type(),
            metrics::Operation::Create,
            metrics::Source::Http,
            handler.create(self.base(), body),
        )
        .await
    }

    /// # Errors
    ///
    /// [`Error::BadRequest`] if `id` does not parse as this resource type's id, else whatever the
    /// [`ResourceService`] answers.
    pub async fn get(&self, endpoint: &str, id: &str) -> Result<Value> {
        let handler = self.handler(endpoint)?;
        self.observe(
            handler.resource_type(),
            metrics::Operation::Get,
            metrics::Source::Http,
            handler.get(self.base(), id),
        )
        .await
    }

    pub async fn replace(&self, endpoint: &str, id: &str, body: Value) -> Result<Value> {
        let handler = self.handler(endpoint)?;
        self.observe(
            handler.resource_type(),
            metrics::Operation::Replace,
            metrics::Source::Http,
            handler.replace(self.base(), id, body),
        )
        .await
    }

    /// `Ok(None)` when the service applied the patch but chose not to return the resource, which
    /// RFC 7644 section 3.5.2 answers with a `204`.
    pub async fn patch(&self, endpoint: &str, id: &str, op: PatchOp) -> Result<Option<Value>> {
        let handler = self.handler(endpoint)?;
        self.observe(
            handler.resource_type(),
            metrics::Operation::Patch,
            metrics::Source::Http,
            handler.patch(self.base(), id, op),
        )
        .await
    }

    pub async fn delete(&self, endpoint: &str, id: &str) -> Result<()> {
        let handler = self.handler(endpoint)?;
        self.observe(
            handler.resource_type(),
            metrics::Operation::Delete,
            metrics::Source::Http,
            handler.delete(id),
        )
        .await
    }

    /// `POST /.search` at the root: every registered resource type at once.
    ///
    /// The result is the concatenation of each handler's answer, in registry order, with
    /// `totalResults` summed. `startIndex` and `count` reach each backend individually rather than
    /// being applied across the whole, so paging a root search is approximate - page a single
    /// resource type's `.search` when it has to be exact.
    ///
    /// # Errors
    ///
    /// Whatever a service answers other than [`Error::MethodNotImplemented`], which is skipped: a
    /// resource type that does not support listing simply contributes nothing.
    pub async fn search_all(&self, query: FilterQuery) -> Result<Value> {
        let mut total = 0;
        let mut resources = vec![];
        for handler in self.handlers.values() {
            // Observed before the skip below: a resource type that cannot list is worth counting.
            let answer = self
                .observe(
                    handler.resource_type(),
                    metrics::Operation::Search,
                    metrics::Source::Http,
                    handler.list(self.base(), query.clone()),
                )
                .await;
            let mut list = match answer {
                Ok(list) => list,
                Err(Error::MethodNotImplemented) => continue,
                Err(e) => return Err(e),
            };
            total += list
                .get("totalResults")
                .and_then(Value::as_u64)
                .unwrap_or_default() as usize;
            if let Some(found) = list.get_mut("Resources").and_then(|r| r.as_array_mut()) {
                resources.append(found);
            }
        }

        let mut combined = ListResponse::new(resources);
        combined.total_results = total;
        resource::to_body(&combined)
    }

    /// `POST /Bulk` from the bytes as received, so that `maxPayloadSize` is checked against what
    /// actually arrived.
    ///
    /// # Errors
    ///
    /// [`Error::MethodNotImplemented`] if bulk is not enabled, [`Error::PayloadTooLarge`] past
    /// `maxPayloadSize`, [`Error::BadRequest`] if the body is not a `BulkRequest`.
    pub async fn bulk_bytes(&self, body: &[u8]) -> Result<BulkResponse> {
        if !self.config.bulk.supported {
            return Err(Error::MethodNotImplemented);
        }
        if let Some(max) = self.config.bulk.max_payload_size
            && body.len() > max as usize
        {
            return Err(Error::PayloadTooLarge(format!(
                "the request body is {} bytes, more than the {max} supported",
                body.len()
            )));
        }
        let request: BulkRequest = serde_json::from_slice(body)
            .map_err(|e| Error::BadRequest(format!("invalid BulkRequest: {e}")))?;
        self.bulk(request).await
    }

    /// `POST /Bulk` from an already-parsed request, for an adapter that does its own
    /// deserialisation. Only [`Self::bulk_bytes`] can enforce `maxPayloadSize`.
    ///
    /// # Errors
    ///
    /// [`Error::MethodNotImplemented`] if bulk is not enabled, [`Error::PayloadTooLarge`] past
    /// `maxOperations`. A failing *operation* is reported inside the response rather than as an
    /// error.
    pub async fn bulk(&self, request: BulkRequest) -> Result<BulkResponse> {
        if !self.config.bulk.supported {
            return Err(Error::MethodNotImplemented);
        }
        bulk::execute(self, request).await
    }

    #[must_use]
    pub fn resource_types(&self) -> ListResponse<ResourceType> {
        ListResponse::new(
            self.handlers
                .values()
                .map(|h| self.resource_type_of(h))
                .collect(),
        )
    }

    /// # Errors
    ///
    /// [`Error::NotFound`] if no resource type goes by that name.
    pub fn resource_type(&self, id: &str) -> Result<ResourceType> {
        self.handlers
            .values()
            .find(|h| h.resource_type() == id)
            .map(|h| self.resource_type_of(h))
            .ok_or_else(|| Error::NotFound(id.to_owned()))
    }

    fn resource_type_of(&self, handler: &Arc<dyn ErasedHandler>) -> ResourceType {
        ResourceType::new(
            handler.resource_type(),
            handler.endpoint(),
            handler.schema_urn(),
        )
        .with_extensions(handler.extensions().into_iter().map(Into::into).collect())
        .at_base(&self.config.location_base)
    }

    /// Every schema the server knows about: each resource type's own, followed by its extensions.
    /// Two resource types sharing an extension advertise it once.
    #[must_use]
    pub fn schemas(&self) -> ListResponse<Schema> {
        let mut seen = BTreeSet::new();
        let mut schemas = vec![];
        for handler in self.handlers.values() {
            let extensions = handler.extensions().into_iter().map(|e| e.schema);
            for schema in std::iter::once(handler.schema()).chain(extensions) {
                if seen.insert(schema.id.clone()) {
                    schemas.push(schema.at_base(&self.config.location_base));
                }
            }
        }
        ListResponse::new(schemas)
    }

    /// # Errors
    ///
    /// [`Error::NotFound`] if no registered resource type declares that URN.
    pub fn schema(&self, urn: &str) -> Result<Schema> {
        self.schemas()
            .resources
            .into_iter()
            .find(|s| s.id == urn)
            .ok_or_else(|| Error::NotFound(urn.to_owned()))
    }

    #[must_use]
    pub fn service_provider_config(&self) -> ServiceProviderConfig {
        let mut config = ServiceProviderConfig::default()
            .with_patch(self.config.enable_patch)
            .with_filter(self.config.filter.supported, self.config.filter.max_results)
            .with_bulk(
                self.config.bulk.supported,
                self.config.bulk.max_operations,
                self.config.bulk.max_payload_size,
            )
            .with_sort(self.config.enable_sort);
        for scheme in &self.config.authentication_schemes {
            config = config.add_authentication_scheme(scheme.clone());
        }
        config
    }
}

pub struct ScimBuilder {
    handlers: Registry,
    config: ScimConfig,
    metrics: Option<Arc<ScimMetrics>>,
}

impl Default for ScimBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ScimBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: Registry::new(),
            config: ScimConfig::default(),
            metrics: None,
        }
    }

    /// Registers the service backing a single SCIM resource type. `R` is normally inferred from the
    /// turbofish - `register::<User, _>(svc)` - and everything needed to route and describe the
    /// resource comes from `R`'s [`Resource`] impl.
    #[must_use]
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

    /// Where the SCIM routes live, e.g. `/scim/v2`. Sets both the mount point and the prefix
    /// `meta.location` is written relative to; use [`Self::location_base`] afterwards to separate
    /// them.
    #[must_use]
    pub fn base_path(mut self, path: &str) -> Self {
        match normalise_path(path) {
            Some(path) => {
                self.config.location_base.clone_from(&path);
                self.config.scope_path = path;
            }
            None => tracing::warn!(
                "'{path}' is not a usable base path - keeping {}",
                self.config.scope_path
            ),
        }
        self
    }

    /// Overrides only the prefix `meta.location` is written relative to, for a host that mounts the
    /// routes inside a scope of its own: the adapter registers under [`Self::base_path`], but a
    /// client sees the host's prefix in front of it.
    #[must_use]
    pub fn location_base(mut self, path: &str) -> Self {
        match normalise_path(path) {
            Some(path) => self.config.location_base = path,
            None => tracing::warn!(
                "'{path}' is not a usable location base - keeping {}",
                self.config.location_base
            ),
        }
        self
    }

    #[must_use]
    pub fn with_patch(mut self, supported: bool) -> Self {
        self.config.enable_patch = supported;
        self
    }

    #[must_use]
    pub fn with_filter(mut self, supported: bool, max_results: Option<u32>) -> Self {
        self.config.filter = FilterConfig::new(supported, max_results);
        self
    }

    #[must_use]
    pub fn with_bulk(
        mut self,
        supported: bool,
        max_operations: Option<u32>,
        max_payload_size: Option<u32>,
    ) -> Self {
        self.config.bulk = BulkConfig {
            supported,
            max_operations,
            max_payload_size,
        };
        self
    }

    #[must_use]
    pub fn with_sort(mut self, supported: bool) -> Self {
        self.config.enable_sort = supported;
        self
    }

    /// Adds a scheme to what `/ServiceProviderConfig` advertises. How a request is actually
    /// authenticated is the adapter's business - or the host's, when it authenticates upstream -
    /// so this only ever describes.
    #[must_use]
    pub fn advertise_authentication_scheme(mut self, scheme: AuthenticationScheme) -> Self {
        self.config.authentication_schemes.push(scheme);
        self
    }

    /// Records the SCIM-layer metrics into `metrics`. Without this nothing is recorded.
    #[must_use]
    pub fn metrics(mut self, metrics: Arc<ScimMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    #[must_use]
    pub fn build(self) -> Scim {
        if let Some(metrics) = &self.metrics {
            metrics.set_resource_types(self.handlers.len());
        }
        Scim {
            handlers: Arc::new(self.handlers),
            config: Arc::new(self.config),
            metrics: self.metrics,
        }
    }
}

#[cfg(test)]
mod tests;
