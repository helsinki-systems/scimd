//! Schema definitions as served from `/Schemas` (RFC 7643 section 7).
//!
//! Writing one out as plain struct literals means naming a dozen `Option` fields per attribute, so
//! [`Attributes`] and [`SubAttributes`] carry builders that start from the defaults RFC 7643 gives
//! and let a schema state only what differs.

use super::meta::Meta;

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Schema {
    pub id: String,
    pub name: String,
    pub description: String,
    pub attributes: Vec<Attributes>,
    pub meta: Meta,
}

impl Schema {
    pub fn new(id: &str, name: &str, description: &str, attributes: Vec<Attributes>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            attributes,
            meta: Meta {
                resource_type: Some("Schema".into()),
                location: Some(format!("{}/Schemas/{id}", crate::SCIM_BASE_PATH)),
                ..Default::default()
            },
        }
    }
}

/// A schema a resource type carries in addition to its core one, namespaced under its own URN -
/// the Enterprise User extension being the canonical case (RFC 7643 section 4.3).
///
/// This is the declaration side; [`super::SchemaExtension`] is the pared-down form `/ResourceTypes`
/// puts on the wire.
#[derive(Debug)]
pub struct Extension {
    pub schema: Schema,
    /// Whether a resource of this type must carry the extension.
    pub required: bool,
}

impl Extension {
    pub fn optional(schema: Schema) -> Self {
        Self {
            schema,
            required: false,
        }
    }

    pub fn required(schema: Schema) -> Self {
        Self {
            schema,
            required: true,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Attributes {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "multiValued")]
    pub multi_valued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(rename = "canonicalValues", skip_serializing_if = "Option::is_none")]
    pub canonical_values: Option<Vec<String>>,
    #[serde(rename = "caseExact", skip_serializing_if = "Option::is_none")]
    pub case_exact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniqueness: Option<String>,
    #[serde(rename = "subAttributes", skip_serializing_if = "Option::is_none")]
    pub sub_attributes: Option<Vec<SubAttributes>>,
    #[serde(rename = "referenceTypes", skip_serializing_if = "Option::is_none")]
    pub reference_types: Option<Vec<String>>,
}

macro_rules! attribute_builders {
    ($t:ty) => {
        impl $t {
            pub fn new(name: &str, type_: &str) -> Self {
                let string_like = matches!(type_, "string" | "reference" | "binary");
                Self {
                    name: name.into(),
                    type_: type_.into(),
                    required: Some(false),
                    case_exact: string_like.then_some(false),
                    mutability: Some("readWrite".into()),
                    returned: Some("default".into()),
                    uniqueness: string_like.then(|| "none".to_string()),
                    ..Default::default()
                }
            }

            pub fn description(mut self, description: &str) -> Self {
                self.description = Some(description.into());
                self
            }

            pub fn multi_valued(mut self) -> Self {
                self.multi_valued = true;
                self
            }

            pub fn required(mut self) -> Self {
                self.required = Some(true);
                self
            }

            pub fn case_exact(mut self) -> Self {
                self.case_exact = Some(true);
                self
            }

            pub fn canonical_values(mut self, values: &[&str]) -> Self {
                self.canonical_values = Some(values.iter().map(|v| (*v).to_string()).collect());
                self
            }

            pub fn reference_types(mut self, types: &[&str]) -> Self {
                self.reference_types = Some(types.iter().map(|v| (*v).to_string()).collect());
                self
            }

            pub fn mutability(mut self, mutability: &str) -> Self {
                self.mutability = Some(mutability.into());
                self
            }

            pub fn returned(mut self, returned: &str) -> Self {
                self.returned = Some(returned.into());
                self
            }

            pub fn uniqueness(mut self, uniqueness: &str) -> Self {
                self.uniqueness = Some(uniqueness.into());
                self
            }
        }
    };
}

attribute_builders!(Attributes);
attribute_builders!(SubAttributes);

impl Attributes {
    pub fn sub_attributes(mut self, sub_attributes: Vec<SubAttributes>) -> Self {
        self.sub_attributes = Some(sub_attributes);
        self
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct SubAttributes {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(rename = "multiValued")]
    pub multi_valued: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(rename = "canonicalValues", skip_serializing_if = "Option::is_none")]
    pub canonical_values: Option<Vec<String>>,
    #[serde(rename = "caseExact", skip_serializing_if = "Option::is_none")]
    pub case_exact: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mutability: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uniqueness: Option<String>,
    #[serde(rename = "referenceTypes", skip_serializing_if = "Option::is_none")]
    pub reference_types: Option<Vec<String>>,
}
