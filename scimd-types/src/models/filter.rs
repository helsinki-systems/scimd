/// The query parameters of a list request (RFC 7644 section 3.4.2), which are also what a
/// `SearchRequest` carries - see [`super::SearchRequest`], which converts into this.
///
/// `start_index` and `count` are passed to the service as given; scimd never slices a result set
/// itself, so a service is free to push them down into its backend or to ignore them.
#[derive(serde::Deserialize, Debug, Default, Clone)]
pub struct FilterQuery {
    pub filter: Option<String>,
    #[serde(rename = "sortBy")]
    pub sort_by: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: Option<String>,
    pub attributes: Option<String>,
    #[serde(rename = "excludedAttributes")]
    pub excluded_attributes: Option<String>,
    /// 1-based, per RFC 7644 section 3.4.2.4.
    #[serde(rename = "startIndex")]
    pub start_index: Option<usize>,
    pub count: Option<usize>,
}
