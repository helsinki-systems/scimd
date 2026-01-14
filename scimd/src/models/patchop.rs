#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct PatchOp {
    // urn:ietf:params:scim:api:messages:2.0:PatchOp
    pub schemas: Vec<String>,

    #[serde(rename = "Operations", default)]
    pub operations: Vec<Operation>,
}

impl Default for PatchOp {
    fn default() -> Self {
        Self {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:PatchOp".to_string()],
            operations: vec![],
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Operation {
    pub op: String,
    pub path: String,
    pub value: Option<serde_json::Value>,
}

impl Operation {
    pub fn add(path: String, value: Option<serde_json::Value>) -> Self {
        Self {
            op: "add".into(),
            path,
            value,
        }
    }

    pub fn remove(path: String) -> Self {
        Self {
            op: "remove".into(),
            path,
            value: None,
        }
    }

    pub fn replace(path: String, value: Option<serde_json::Value>) -> Self {
        Self {
            op: "replace".into(),
            path,
            value,
        }
    }
}
