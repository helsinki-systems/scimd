#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone)]
pub struct Meta {
    #[serde(rename = "resourceType", skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(rename = "lastModified", skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Schema {
    pub id: String,
    pub name: String,
    pub description: String,
    pub attributes: Vec<Attributes>,
    pub meta: Meta,
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
