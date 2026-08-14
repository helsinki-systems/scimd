//! The generic resource machinery.
//!
//! RFC 7643 treats `User` and `Group` as two *predefined* resource types; a provider is free to
//! define its own and advertise them via `/ResourceTypes` and `/Schemas`. This module is what makes
//! that possible here: a resource type is described by [`Resource`], its behaviour is supplied by
//! [`ResourceService`], and [`TypedHandler`] erases the two into the object-safe [`ErasedHandler`]
//! so that handlers for different resource types can share one registry.

use std::marker::PhantomData;

use async_trait::async_trait;
use serde_json::Value;

use crate::models::{FilterQuery, ListResponse, Meta, PatchOp, Schema, ValOrEmpty};
use crate::{Error, Result, SCIM_BASE_PATH};

/// Describes what makes a type a SCIM resource: its type name, endpoint, schema URN, and how to
/// read and write its identity and metadata.
///
/// `Id` is an associated type rather than a fixed `Uuid` because RFC 7643 defines `id` as an opaque
/// string; a provider keyed by UUIDs uses `type Id = Uuid`, one keyed by anything else says so. It
/// is parsed out of the request path, hence the `FromStr` bound, and rendered back into
/// `meta.location`, hence `Display`.
pub trait Resource: serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static {
    type Id: std::str::FromStr<Err: std::fmt::Display> + std::fmt::Display + Send + Sync + 'static;

    /// The `ResourceType` name, e.g. `"User"`.
    const RESOURCE_TYPE: &'static str;
    /// The endpoint relative to the SCIM base path, e.g. `"/Users"`.
    const ENDPOINT: &'static str;
    /// The schema URN, e.g. `"urn:ietf:params:scim:schemas:core:2.0:User"`.
    const SCHEMA: &'static str;

    fn id(&self) -> Option<&Self::Id>;

    fn meta(&self) -> Option<&Meta>;

    fn set_meta(&mut self, meta: Meta);

    fn schema() -> Schema;
}

/// The six operations a SCIM resource endpoint can expose.
///
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
/// live in one registry. Identity crosses the boundary as `&str` and payloads as [`Value`]; the
/// typed conversion happens in [`TypedHandler`].
#[async_trait]
pub(crate) trait ErasedHandler: Send + Sync {
    fn endpoint(&self) -> &'static str;
    fn resource_type(&self) -> &'static str;
    fn schema_urn(&self) -> &'static str;
    fn schema(&self) -> Schema;

    async fn list(&self, filter: FilterQuery) -> Result<Value>;
    async fn create(&self, body: Value) -> Result<Value>;
    async fn get(&self, id: &str) -> Result<Value>;
    async fn replace(&self, id: &str, body: Value) -> Result<Value>;
    async fn patch(&self, id: &str, op: PatchOp) -> Result<Option<Value>>;
    async fn delete(&self, id: &str) -> Result<()>;
}

pub(crate) struct TypedHandler<R, S> {
    service: S,
    _marker: PhantomData<fn() -> R>,
}

impl<R, S> TypedHandler<R, S> {
    pub(crate) fn new(service: S) -> Self {
        Self {
            service,
            _marker: PhantomData,
        }
    }
}

/// Fills in the parts of `meta` the service left unset. Values the service did supply - notably
/// `created` and `lastModified` - are never overwritten.
pub(crate) fn finalize<R: Resource>(resource: &mut R) {
    let mut meta = resource.meta().cloned().unwrap_or_default();
    if meta.resource_type.is_none() {
        meta.resource_type = Some(R::RESOURCE_TYPE.to_owned());
    }
    if meta.location.is_none()
        && let Some(id) = resource.id()
    {
        meta.location = Some(format!("{SCIM_BASE_PATH}{}/{id}", R::ENDPOINT));
    }
    resource.set_meta(meta);
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
fn to_body<T: serde::Serialize>(value: &T) -> Result<Value> {
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

    async fn list(&self, filter: FilterQuery) -> Result<Value> {
        let mut list = self.service.list(filter).await?;
        for resource in &mut list.resources {
            finalize(resource);
        }
        to_body(&list)
    }

    async fn create(&self, body: Value) -> Result<Value> {
        let mut created = self.service.create(from_body::<R>(body)?).await?;
        finalize(&mut created);
        to_body(&created)
    }

    async fn get(&self, id: &str) -> Result<Value> {
        let mut resource = self.service.get(parse_id::<R>(id)?).await?;
        finalize(&mut resource);
        to_body(&resource)
    }

    async fn replace(&self, id: &str, body: Value) -> Result<Value> {
        let mut replaced = self
            .service
            .replace(parse_id::<R>(id)?, from_body::<R>(body)?)
            .await?;
        finalize(&mut replaced);
        to_body(&replaced)
    }

    async fn patch(&self, id: &str, op: PatchOp) -> Result<Option<Value>> {
        match self.service.patch(parse_id::<R>(id)?, op).await? {
            ValOrEmpty::Val(mut patched) => {
                finalize(&mut patched);
                Ok(Some(to_body(&patched)?))
            }
            ValOrEmpty::Empty => Ok(None),
        }
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.service.delete(parse_id::<R>(id)?).await
    }
}
