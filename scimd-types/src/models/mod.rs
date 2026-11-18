pub mod bulk;
pub mod enterprise_user;
mod error;
pub mod filter;
pub mod group;
pub mod list;
pub mod meta;
pub mod patchop;
pub mod resource_type;
pub mod schema;
mod schemas;
pub mod search;
pub mod service_provider_config;
pub mod user;

pub use bulk::{BulkOperation, BulkOperationResult, BulkRequest, BulkResponse};
// The wire document, not the error enum; public only because `Error::to_scim_error` returns it.
#[doc(hidden)]
pub use error::Error;
pub use filter::FilterQuery;
pub use group::Group;
pub use list::ListResponse;
pub use meta::Meta;
pub use patchop::{PatchOp, normalize_attr};
pub use resource_type::{ResourceType, SchemaExtension};
pub use schema::{Attributes, Extension, Schema, SubAttributes};
pub use search::SearchRequest;
pub use service_provider_config::ServiceProviderConfig;
pub use user::User;

#[derive(serde::Serialize, Debug)]
pub enum ValOrEmpty<T> {
    Val(T),
    Empty,
}

/// An optional boolean that may also arrive as the string `"true"` or `"false"` in any case.
/// Entra ID sends `"True"`.
pub(crate) fn lenient_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    use serde::de::{Error as _, Unexpected};

    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Lenient {
        Bool(bool),
        String(String),
    }

    match Option::<Lenient>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Lenient::Bool(b)) => Ok(Some(b)),
        Some(Lenient::String(s)) if s.eq_ignore_ascii_case("true") => Ok(Some(true)),
        Some(Lenient::String(s)) if s.eq_ignore_ascii_case("false") => Ok(Some(false)),
        Some(Lenient::String(s)) => Err(D::Error::invalid_value(
            Unexpected::Str(&s),
            &"a boolean, or \"true\" or \"false\"",
        )),
    }
}
