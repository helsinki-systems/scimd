#[derive(serde::Deserialize, Debug, Default)]
pub struct FilterQuery {
    pub filter: Option<String>,
    #[serde(rename = "sortBy")]
    pub sort_by: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: Option<String>,
    pub arrtibutes: Option<String>,
}
