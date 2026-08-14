//! The `/Bulk` request and response messages (RFC 7644 section 3.7).

use serde_json::Value;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct BulkRequest {
    // urn:ietf:params:scim:api:messages:2.0:BulkRequest
    #[serde(default)]
    pub schemas: Vec<String>,
    /// Stop once this many operations have failed. Absent means run the whole batch regardless.
    #[serde(
        rename = "failOnErrors",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub fail_on_errors: Option<usize>,
    #[serde(rename = "Operations", default)]
    pub operations: Vec<BulkOperation>,
}

impl Default for BulkRequest {
    fn default() -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:BulkRequest".into()],
            fail_on_errors: None,
            operations: vec![],
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct BulkOperation {
    pub method: String,
    pub path: String,
    #[serde(rename = "bulkId", default, skip_serializing_if = "Option::is_none")]
    pub bulk_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct BulkResponse {
    // urn:ietf:params:scim:api:messages:2.0:BulkResponse
    pub schemas: Vec<String>,
    #[serde(rename = "Operations")]
    pub operations: Vec<BulkOperationResult>,
}

impl BulkResponse {
    pub fn new(operations: Vec<BulkOperationResult>) -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:BulkResponse".into()],
            operations,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct BulkOperationResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    pub method: String,
    #[serde(rename = "bulkId", default, skip_serializing_if = "Option::is_none")]
    pub bulk_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub status: String,
    /// The SCIM error document, present only when this operation failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

impl BulkOperationResult {
    pub fn new(method: &str, bulk_id: Option<String>, status: actix_web::http::StatusCode) -> Self {
        Self {
            location: None,
            method: method.to_string(),
            bulk_id,
            version: None,
            status: status.as_u16().to_string(),
            response: None,
        }
    }

    pub fn with_location(mut self, location: Option<String>) -> Self {
        self.location = location;
        self
    }

    pub fn with_response(mut self, response: Value) -> Self {
        self.response = Some(response);
        self
    }
}
