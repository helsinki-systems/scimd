#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceProviderConfig {
    /// MUST be ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"]
    pub schemas: Vec<String>,

    #[serde(rename = "documentationUri", skip_serializing_if = "Option::is_none")]
    pub documentation_uri: Option<String>,

    patch: Supported,
    bulk: BulkConfig,
    filter: FilterConfig,
    #[serde(rename = "changePassword")]
    change_password: Supported,
    sort: Supported,
    etag: Supported,

    #[serde(
        rename = "authenticationSchemes",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub authentication_schemes: Vec<AuthenticationScheme>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<super::meta::Meta>,
}

impl Default for ServiceProviderConfig {
    fn default() -> Self {
        Self {
            schemas: vec![
                "urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig".to_string(),
            ],
            documentation_uri: None,
            patch: Supported::default(),
            bulk: BulkConfig::default(),
            filter: FilterConfig::default(),
            change_password: Supported::default(),
            sort: Supported::default(),
            etag: Supported::default(),
            authentication_schemes: vec![],
            meta: None,
        }
    }
}

impl ServiceProviderConfig {
    pub fn with_patch(mut self, supported: bool) -> Self {
        self.patch = Supported::new(supported);
        self
    }

    pub fn with_filter(mut self, supported: bool, max_results: Option<u32>) -> Self {
        self.filter = FilterConfig::new(supported, max_results);
        self
    }

    pub fn with_bulk(
        mut self,
        supported: bool,
        max_operations: Option<u32>,
        max_payload_size: Option<u32>,
    ) -> Self {
        self.bulk = BulkConfig {
            supported,
            max_operations,
            max_payload_size,
        };
        self
    }

    pub fn with_sort(mut self, supported: bool) -> Self {
        self.sort = Supported::new(supported);
        self
    }

    pub fn add_authentication_scheme(mut self, authentication: AuthenticationScheme) -> Self {
        self.authentication_schemes.push(authentication);
        self
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct Supported {
    supported: bool,
}

impl Supported {
    pub fn new(supported: bool) -> Self {
        Self { supported }
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct BulkConfig {
    pub supported: bool,
    #[serde(rename = "maxOperations", skip_serializing_if = "Option::is_none")]
    pub max_operations: Option<u32>,
    #[serde(rename = "maxPayloadSize", skip_serializing_if = "Option::is_none")]
    pub max_payload_size: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct FilterConfig {
    pub supported: bool,
    #[serde(rename = "maxResults", skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
}

impl FilterConfig {
    pub fn new(supported: bool, max_results: Option<u32>) -> Self {
        Self {
            supported,
            max_results,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthenticationScheme {
    #[serde(rename = "type")]
    pub r#type: String,

    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "specUri", skip_serializing_if = "Option::is_none")]
    pub spec_uri: Option<String>,
    #[serde(rename = "documentationUri", skip_serializing_if = "Option::is_none")]
    pub documentation_uri: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<bool>,
}

impl AuthenticationScheme {
    pub fn bearer() -> Self {
        Self {
            r#type: "oauthbearertoken".into(),
            name: "OAuth Bearer Token".into(),
            description: Some("Authentication scheme using the OAuth Bearer Token Standard".into()),
            spec_uri: Some("http://www.rfc-editor.org/info/rfc6750".into()),
            documentation_uri: None,
            primary: Some(true),
        }
    }
}
