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
pub(crate) mod service_provider_config;
pub mod user;

pub use bulk::{BulkOperation, BulkOperationResult, BulkRequest, BulkResponse};
pub(crate) use error::Error;
pub use filter::FilterQuery;
pub use group::Group;
pub use list::ListResponse;
pub use meta::Meta;
pub use patchop::PatchOp;
pub use resource_type::{ResourceType, SchemaExtension};
pub use schema::{Attributes, Extension, Schema, SubAttributes};
pub use search::SearchRequest;
pub(crate) use service_provider_config::ServiceProviderConfig;
pub use user::User;

#[derive(serde::Serialize, Debug)]
pub enum ValOrEmpty<T> {
    Val(T),
    Empty,
}
