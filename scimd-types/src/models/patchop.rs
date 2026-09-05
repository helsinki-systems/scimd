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

/// Reduce a patch path to the form a service can match on.
///
/// RFC 7644 section 3.5.2 lets a client write the fully qualified
/// `urn:ietf:params:scim:schemas:core:2.0:User:userName` wherever it could write `userName`, and
/// RFC 7643 section 2.1 makes attribute names case-insensitive. Both are handled once, here, so
/// that no service has to remember either.
///
/// The URN is dropped rather than kept: an extension attribute reduces to its own name, which is
/// unambiguous in practice and keeps a service from having to spell out URNs to match on.
pub fn normalize_attr(path: &str) -> String {
    let attr = match path.rfind(':') {
        Some(colon) => &path[colon + 1..],
        None => path,
    };
    attr.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::normalize_attr;

    #[test]
    fn a_fully_qualified_patch_path_means_the_same_as_the_short_one() {
        assert_eq!(
            normalize_attr("urn:ietf:params:scim:schemas:core:2.0:User:userName"),
            normalize_attr("userName")
        );
        assert_eq!(normalize_attr("userName"), "username");
    }

    #[test]
    fn a_sub_attribute_keeps_its_dotted_path() {
        assert_eq!(normalize_attr("name.givenName"), "name.givenname");
    }
}
