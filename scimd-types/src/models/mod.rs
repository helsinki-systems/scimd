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
