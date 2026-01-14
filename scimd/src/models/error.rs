#[derive(serde::Serialize, Debug, Default)]
pub(crate) struct Error {
    // urn:ietf:params:scim:api:messages:2.0:Error
    schemas: Vec<String>,
    status: String,
    #[serde(rename = "scimType", skip_serializing_if = "Option::is_none")]
    scim_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

impl Error {
    pub fn new(status: actix_web::http::StatusCode) -> Self {
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
}
