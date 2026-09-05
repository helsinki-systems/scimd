use super::meta::Meta;

/// A `ResourceType` as served from `/ResourceTypes` (RFC 7643 section 6).
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ResourceType {
    // urn:ietf:params:scim:schemas:core:2.0:ResourceType
    pub schemas: Vec<String>,
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub endpoint: String,
    pub schema: String,
    #[serde(rename = "schemaExtensions", skip_serializing_if = "Vec::is_empty")]
    pub schema_extensions: Vec<SchemaExtension>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

impl ResourceType {
    pub fn new(resource_type: &str, endpoint: &str, schema: &str) -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:schemas:core:2.0:ResourceType".into()],
            id: resource_type.into(),
            name: resource_type.into(),
            description: None,
            endpoint: endpoint.into(),
            schema: schema.into(),
            schema_extensions: vec![],
            meta: Some(Meta {
                resource_type: Some("ResourceType".into()),
                ..Default::default()
            }),
        }
    }

    /// Points `meta.location` at this resource type on a server serving `/ResourceTypes` under
    /// `base`. Left unset by [`ResourceType::new`] for the reason given on [`super::Schema::new`].
    #[must_use]
    pub fn at_base(mut self, base: &str) -> Self {
        let location = format!("{base}/ResourceTypes/{}", self.id);
        self.meta.get_or_insert_default().location = Some(location);
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_extensions(mut self, extensions: Vec<SchemaExtension>) -> Self {
        self.schema_extensions = extensions;
        self
    }
}

/// An extension as `/ResourceTypes` advertises it: the URN and whether it is mandatory. The schema
/// itself is served from `/Schemas`; see [`super::Extension`] for the declaration side.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SchemaExtension {
    pub schema: String,
    pub required: bool,
}

impl From<super::Extension> for SchemaExtension {
    fn from(extension: super::Extension) -> Self {
        Self {
            schema: extension.schema.id,
            required: extension.required,
        }
    }
}
