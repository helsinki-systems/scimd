use super::FilterQuery;

/// The body of a `POST .../.search` (RFC 7644 section 3.4.3), which carries exactly what a `GET`
/// list request carries in its query string
#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct SearchRequest {
    // urn:ietf:params:scim:api:messages:2.0:SearchRequest
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attributes: Option<String>,
    #[serde(
        rename = "excludedAttributes",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub excluded_attributes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(rename = "sortBy", default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<String>,
    #[serde(rename = "sortOrder", default, skip_serializing_if = "Option::is_none")]
    pub sort_order: Option<String>,
    #[serde(
        rename = "startIndex",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub start_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

impl Default for SearchRequest {
    fn default() -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:SearchRequest".into()],
            attributes: None,
            excluded_attributes: None,
            filter: None,
            sort_by: None,
            sort_order: None,
            start_index: None,
            count: None,
        }
    }
}

impl From<SearchRequest> for FilterQuery {
    fn from(request: SearchRequest) -> Self {
        Self {
            filter: request.filter,
            sort_by: request.sort_by,
            sort_order: request.sort_order,
            attributes: request.attributes,
            excluded_attributes: request.excluded_attributes,
            start_index: request.start_index,
            count: request.count,
        }
    }
}
