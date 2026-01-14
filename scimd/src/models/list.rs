#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ListResponse<T> {
    // urn:ietf:params:scim:api:messages:2.0:ListResponse
    pub schemas: Vec<String>,
    #[serde(rename = "totalResults")]
    pub total_results: usize,
    #[serde(rename = "Resources", skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<T>,
}

impl<T> ListResponse<T> {
    pub fn new(resources: Vec<T>) -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:ListResponse".into()],
            total_results: resources.len(),
            resources,
        }
    }
}
