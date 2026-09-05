/// The SCIM error *document* as it goes on the wire (RFC 7644 section 3.12), which is what
/// [`crate::Error::to_scim_error`] renders into. Not to be confused with [`crate::Error`], the
/// enum a service returns.
#[derive(serde::Serialize, Debug, Default)]
pub struct Error {
    // urn:ietf:params:scim:api:messages:2.0:Error
    schemas: Vec<String>,
    status: String,
    #[serde(rename = "scimType", skip_serializing_if = "Option::is_none")]
    scim_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl Error {
    pub fn new(status: http::StatusCode) -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:Error".into()],
            status: status.as_u16().to_string(),
            scim_type: None,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail;
        self
    }

    pub fn with_scim_type(mut self, scim_type: Option<&str>) -> Self {
        self.scim_type = scim_type.map(str::to_string);
        self
    }
}
