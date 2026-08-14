/// Field order follows RFC 7644 section 3.4.2 and is meaningful on the wire, since serde_json
/// runs with `preserve_order`.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct ListResponse<T> {
    // urn:ietf:params:scim:api:messages:2.0:ListResponse
    pub schemas: Vec<String>,
    #[serde(rename = "totalResults")]
    pub total_results: usize,
    #[serde(rename = "itemsPerPage", skip_serializing_if = "Option::is_none")]
    pub items_per_page: Option<usize>,
    #[serde(rename = "startIndex", skip_serializing_if = "Option::is_none")]
    pub start_index: Option<usize>,
    #[serde(rename = "Resources", skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<T>,
}

impl<T> ListResponse<T> {
    pub fn new(resources: Vec<T>) -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:ListResponse".into()],
            total_results: resources.len(),
            items_per_page: None,
            start_index: None,
            resources,
        }
    }

    pub fn paginated(resources: Vec<T>, total_results: usize, start_index: usize) -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:ListResponse".into()],
            total_results,
            items_per_page: Some(resources.len()),
            start_index: Some(start_index),
            resources,
        }
    }
}
