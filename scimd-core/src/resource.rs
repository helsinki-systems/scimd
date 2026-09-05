//! The generic resource machinery: a resource type is described by [`Resource`], its behaviour is
//! supplied by [`ResourceService`], and [`TypedHandler`] erases the two into the object-safe
//! [`ErasedHandler`] so that handlers for different resource types share one registry.

use std::marker::PhantomData;

use async_trait::async_trait;
use serde_json::Value;

use scimd_types::models::{Extension, FilterQuery, ListResponse, PatchOp, Schema, ValOrEmpty};
use scimd_types::{Error, Resource, Result};

/// Every method defaults to [`Error::MethodNotImplemented`], so an implementor overrides only what
/// their product actually supports and the rest surface as `501 Not Implemented`.
#[async_trait]
pub trait ResourceService<R: Resource>: Send + Sync {
    async fn list(&self, _filter: FilterQuery) -> Result<ListResponse<R>> {
        Err(Error::MethodNotImplemented)
    }

    async fn create(&self, _resource: R) -> Result<R> {
        Err(Error::MethodNotImplemented)
    }

    async fn get(&self, _id: R::Id) -> Result<R> {
        Err(Error::MethodNotImplemented)
    }

    async fn replace(&self, _id: R::Id, _resource: R) -> Result<R> {
        Err(Error::MethodNotImplemented)
    }

    async fn patch(&self, _id: R::Id, _op: PatchOp) -> Result<ValOrEmpty<R>> {
        Err(Error::MethodNotImplemented)
    }

    async fn delete(&self, _id: R::Id) -> Result<()> {
        Err(Error::MethodNotImplemented)
    }
}

/// The object-safe view of a [`ResourceService`], so that services for different resource types can
/// live in one registry. Identity crosses the boundary as `&str` and payloads as [`Value`]; `base`
/// is what `meta.location` is written relative to.
#[async_trait]
pub(crate) trait ErasedHandler: Send + Sync {
    fn endpoint(&self) -> &'static str;
    fn resource_type(&self) -> &'static str;
    fn schema_urn(&self) -> &'static str;
    fn schema(&self) -> Schema;
    fn extensions(&self) -> Vec<Extension>;

    async fn list(&self, base: &str, filter: FilterQuery) -> Result<Value>;
    async fn create(&self, base: &str, body: Value) -> Result<Value>;
    async fn get(&self, base: &str, id: &str) -> Result<Value>;
    async fn replace(&self, base: &str, id: &str, body: Value) -> Result<Value>;
    async fn patch(&self, base: &str, id: &str, op: PatchOp) -> Result<Option<Value>>;
    async fn delete(&self, id: &str) -> Result<()>;
}

pub(crate) struct TypedHandler<R, S> {
    service: S,
    extension_urns: Vec<String>,
    _marker: PhantomData<fn() -> R>,
}

impl<R: Resource, S> TypedHandler<R, S> {
    pub(crate) fn new(service: S) -> Self {
        Self {
            service,
            extension_urns: R::extensions()
                .into_iter()
                .map(|extension| extension.schema.id)
                .collect(),
            _marker: PhantomData,
        }
    }

    fn body(&self, base: &str, resource: &mut R) -> Result<Value> {
        finalize(resource, base);
        let mut value = to_body(resource)?;
        reconcile_schemas(&mut value, R::SCHEMA, &self.extension_urns);
        Ok(value)
    }
}

/// Fills in the parts of `meta` the service left unset. Values the service did supply - notably
/// `created` and `lastModified` - are never overwritten.
pub(crate) fn finalize<R: Resource>(resource: &mut R, base: &str) {
    let mut meta = resource.meta().cloned().unwrap_or_default();
    if meta.resource_type.is_none() {
        meta.resource_type = Some(R::RESOURCE_TYPE.to_owned());
    }
    if meta.location.is_none()
        && let Some(id) = resource.id()
    {
        meta.location = Some(format!("{base}{}/{id}", R::ENDPOINT));
    }
    resource.set_meta(meta);
}

/// RFC 7643 requires a resource to name in `schemas` every schema it actually carries. A service
/// that fills in an extension but forgets to declare it produces a body real IdPs reject, so the
/// missing URNs are added here. Additive only - a URN the service listed is never removed, and an
/// extension whose data is absent is never announced.
fn reconcile_schemas(value: &mut Value, core: &str, extension_urns: &[String]) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    let mut carried = vec![core];
    carried.extend(
        extension_urns
            .iter()
            .filter(|urn| object.get(urn.as_str()).is_some_and(|v| !v.is_null()))
            .map(String::as_str),
    );

    let declared = object.get("schemas").and_then(Value::as_array);
    let missing: Vec<Value> = carried
        .into_iter()
        .filter(|urn| declared.is_none_or(|d| !d.iter().any(|v| v.as_str() == Some(urn))))
        .map(Value::from)
        .collect();
    if missing.is_empty() {
        return;
    }

    match object.get_mut("schemas").and_then(Value::as_array_mut) {
        Some(schemas) => schemas.extend(missing),
        None => {
            object.insert("schemas".to_string(), Value::Array(missing));
        }
    }
}

fn parse_id<R: Resource>(id: &str) -> Result<R::Id> {
    id.parse::<R::Id>().map_err(|e| {
        Error::BadRequest(format!(
            "'{id}' is not a valid {} id: {e}",
            R::RESOURCE_TYPE
        ))
    })
}

/// A malformed request body is the client's fault, so it maps to `400` rather than `500`.
fn from_body<R: Resource>(body: Value) -> Result<R> {
    serde_json::from_value(body)
        .map_err(|e| Error::BadRequest(format!("invalid {} payload: {e}", R::RESOURCE_TYPE)))
}

/// Failing to serialize our own response is our fault -> 500.
pub(crate) fn to_body<T: serde::Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| Error::InternalError(e.into()))
}

#[async_trait]
impl<R, S> ErasedHandler for TypedHandler<R, S>
where
    R: Resource,
    S: ResourceService<R>,
{
    fn endpoint(&self) -> &'static str {
        R::ENDPOINT
    }

    fn resource_type(&self) -> &'static str {
        R::RESOURCE_TYPE
    }

    fn schema_urn(&self) -> &'static str {
        R::SCHEMA
    }

    fn schema(&self) -> Schema {
        R::schema()
    }

    fn extensions(&self) -> Vec<Extension> {
        R::extensions()
    }

    async fn list(&self, base: &str, filter: FilterQuery) -> Result<Value> {
        let mut list = self.service.list(filter).await?;
        for resource in &mut list.resources {
            finalize(resource, base);
        }
        let mut value = to_body(&list)?;
        if let Some(resources) = value.get_mut("Resources").and_then(Value::as_array_mut) {
            for resource in resources {
                reconcile_schemas(resource, R::SCHEMA, &self.extension_urns);
            }
        }
        Ok(value)
    }

    async fn create(&self, base: &str, body: Value) -> Result<Value> {
        let mut created = self.service.create(from_body::<R>(body)?).await?;
        self.body(base, &mut created)
    }

    async fn get(&self, base: &str, id: &str) -> Result<Value> {
        let mut resource = self.service.get(parse_id::<R>(id)?).await?;
        self.body(base, &mut resource)
    }

    async fn replace(&self, base: &str, id: &str, body: Value) -> Result<Value> {
        let mut replaced = self
            .service
            .replace(parse_id::<R>(id)?, from_body::<R>(body)?)
            .await?;
        self.body(base, &mut replaced)
    }

    async fn patch(&self, base: &str, id: &str, op: PatchOp) -> Result<Option<Value>> {
        match self.service.patch(parse_id::<R>(id)?, op).await? {
            ValOrEmpty::Val(mut patched) => Ok(Some(self.body(base, &mut patched)?)),
            ValOrEmpty::Empty => Ok(None),
        }
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.service.delete(parse_id::<R>(id)?).await
    }
}
