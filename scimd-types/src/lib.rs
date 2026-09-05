//! The SCIM 2.0 data model (RFC 7643 and the message schemas of RFC 7644), free of any web
//! framework: the documents that go on the wire, the [`Error`] a service answers with, and the
//! [`Resource`] trait that describes a type to the layers above.

mod error;
pub mod models;
mod resource;

pub use error::{Error, Result};
pub use resource::Resource;

pub use http::StatusCode;

pub const SCIM_BASE_PATH: &str = "/v2";
