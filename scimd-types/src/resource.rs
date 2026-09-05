use crate::models::{Extension, Meta, Schema};

/// What makes a type a SCIM resource.
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

    fn extensions() -> Vec<Extension> {
        vec![]
    }
}
