//! Parsing and evaluation of SCIM filters (RFC 7644 section 3.4.2.2).
//!
//! ```
//! let filter = scimd_core::filter::parse(r#"userName eq "bjensen""#).expect("a valid filter");
//! assert!(filter.matches(&serde_json::json!({ "userName": "bjensen" })));
//! ```
//!
//! Attribute names always match case-insensitively (RFC 7643 section 2.1). String values do too,
//! RFC 7643 section 7's default, except where [`Filter::matches_resource`] finds the attribute
//! declared `caseExact` in the resource type's schema; [`Filter::matches`] has no schema to ask.
//! A comparison whose operands have no ordering, such as `active gt true`, is not a match rather
//! than an error.

mod eval;
mod parser;

#[cfg(test)]
mod tests;

pub use parser::{MAX_DEPTH, MAX_LENGTH, parse};

use crate::Resource;
use crate::models::{FilterQuery, ListResponse};

/// Answers a list request from a collection the service holds whole: keeps what the query's
/// `filter` matches, then pages what is left by `startIndex` and `count`. `totalResults` counts
/// every match, not just the page.
///
/// A backend that can filter or page in its own store should do that instead and answer with
/// [`ListResponse::paginated`] itself.
///
/// # Errors
///
/// [`crate::Error::InvalidFilter`] if the filter does not parse, and whatever
/// [`Filter::matches_resource`] raises.
pub fn list_response<R: Resource>(
    resources: Vec<R>,
    query: &FilterQuery,
) -> crate::Result<ListResponse<R>> {
    let matched = match query.filter.as_deref().map(parse).transpose()? {
        None => resources,
        Some(filter) => {
            let schemas = eval::Schemas::of::<R>();
            let mut matched = Vec::with_capacity(resources.len());
            for resource in resources {
                if filter.matches_in(&resource, &schemas)? {
                    matched.push(resource);
                }
            }
            matched
        }
    };

    // RFC 7644 section 3.4.2.4: `startIndex` counts from 1, and anything below 1 counts as 1.
    let total = matched.len();
    let start_index = query.start_index.unwrap_or(1).max(1);
    let page: Vec<R> = matched
        .into_iter()
        .skip(start_index - 1)
        .take(query.count.unwrap_or(usize::MAX))
        .collect();

    Ok(ListResponse::paginated(page, total, start_index))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
    Not(Box<Filter>),
    Present(AttrPath),
    Compare {
        attr: AttrPath,
        op: CompareOp,
        value: CompareValue,
    },
    ValuePath {
        attr: AttrPath,
        filter: Box<Filter>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrPath {
    pub urn: Option<String>,
    pub attr: String,
    pub sub_attr: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Co,
    Sw,
    Ew,
    Gt,
    Lt,
    Ge,
    Le,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompareValue {
    String(String),
    Number(serde_json::Number),
    Bool(bool),
    Null,
}

impl Filter {
    /// Whether a resource, as JSON, satisfies this filter. Without a schema every string value
    /// matches case-insensitively.
    #[must_use]
    pub fn matches(&self, resource: &serde_json::Value) -> bool {
        eval::matches(self, resource, eval::Scope::Unknown)
    }

    /// Whether a typed resource satisfies this filter, honouring the `caseExact` its resource
    /// type's schema, its extensions and RFC 7643 section 3.1's common attributes declare.
    ///
    /// # Errors
    ///
    /// [`crate::Error::InternalError`] if the resource cannot be serialised.
    pub fn matches_resource<R: Resource>(&self, resource: &R) -> crate::Result<bool> {
        self.matches_in(resource, &eval::Schemas::of::<R>())
    }

    fn matches_in<R: Resource>(
        &self,
        resource: &R,
        schemas: &eval::Schemas,
    ) -> crate::Result<bool> {
        let value = serde_json::to_value(resource).map_err(|e| {
            anyhow::Error::new(e).context("serialising a resource to match a filter against it")
        })?;
        Ok(eval::matches(self, &value, eval::Scope::Resource(schemas)))
    }
}

impl CompareOp {
    fn from_keyword(word: &str) -> Option<Self> {
        [
            ("eq", Self::Eq),
            ("ne", Self::Ne),
            ("co", Self::Co),
            ("sw", Self::Sw),
            ("ew", Self::Ew),
            ("gt", Self::Gt),
            ("lt", Self::Lt),
            ("ge", Self::Ge),
            ("le", Self::Le),
        ]
        .into_iter()
        .find(|(keyword, _)| word.eq_ignore_ascii_case(keyword))
        .map(|(_, op)| op)
    }
}

impl std::fmt::Display for AttrPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(urn) = &self.urn {
            write!(f, "{urn}:")?;
        }
        f.write_str(&self.attr)?;
        if let Some(sub_attr) = &self.sub_attr {
            write!(f, ".{sub_attr}")?;
        }
        Ok(())
    }
}
