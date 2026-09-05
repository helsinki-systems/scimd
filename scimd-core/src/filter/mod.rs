//! Parsing and evaluation of SCIM filters (RFC 7644 section 3.4.2.2).
//!
//! ```
//! let filter = scimd_core::filter::parse(r#"userName eq "bjensen""#).expect("a valid filter");
//! assert!(filter.matches(&serde_json::json!({ "userName": "bjensen" })));
//! ```
//!
//! Evaluation reads the JSON, not the resource's schema, with two consequences. Values are matched
//! case-insensitively as well as attribute names, because RFC 7643 section 7 defaults `caseExact`
//! to false and nothing here knows which attributes declare otherwise. And a comparison whose
//! operands have no ordering, such as `active gt true`, is not a match rather than an error.

mod eval;
mod parser;

#[cfg(test)]
mod tests;

pub use parser::{MAX_DEPTH, MAX_LENGTH, parse};

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
    /// Whether a resource, as JSON, satisfies this filter.
    #[must_use]
    pub fn matches(&self, resource: &serde_json::Value) -> bool {
        eval::matches(self, resource)
    }

    /// [`Filter::matches`] for a service that holds its resources typed rather than as JSON.
    ///
    /// # Errors
    ///
    /// [`crate::Error::InternalError`] if the resource cannot be serialised.
    pub fn matches_resource<T: serde::Serialize>(&self, resource: &T) -> crate::Result<bool> {
        let value = serde_json::to_value(resource).map_err(|e| {
            anyhow::Error::new(e).context("serialising a resource to match a filter against it")
        })?;
        Ok(self.matches(&value))
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
