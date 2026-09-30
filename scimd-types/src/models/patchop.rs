#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(try_from = "RawPatchOp")]
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

/// A `PatchOp` as it arrives, where `path` may be missing.
#[derive(serde::Deserialize)]
struct RawPatchOp {
    schemas: Vec<String>,
    #[serde(rename = "Operations", default)]
    operations: Vec<RawOperation>,
}

#[derive(serde::Deserialize)]
struct RawOperation {
    op: String,
    path: Option<String>,
    value: Option<serde_json::Value>,
}

impl TryFrom<RawPatchOp> for PatchOp {
    type Error = String;

    fn try_from(raw: RawPatchOp) -> Result<Self, Self::Error> {
        let mut operations = vec![];
        for operation in raw.operations {
            match operation.path {
                Some(path) => operations.push(Operation {
                    op: operation.op,
                    path,
                    value: operation.value,
                }),
                None => expand_pathless(operation.op, operation.value, &mut operations)?,
            }
        }
        Ok(Self {
            schemas: raw.schemas,
            operations,
        })
    }
}

/// RFC 7644 section 3.5.2: an `add` or `replace` without `path` targets the resource itself, and
/// its value is an object of attributes. It is split into one operation per attribute so that a
/// service only ever sees operations with a path. An extension's attributes arrive nested under the
/// extension's URN and get the fully qualified path `<urn>:<attribute>`.
fn expand_pathless(
    op: String,
    value: Option<serde_json::Value>,
    operations: &mut Vec<Operation>,
) -> Result<(), String> {
    if op.eq_ignore_ascii_case("remove") {
        return Err("a remove operation needs a path".into());
    }
    let Some(serde_json::Value::Object(attributes)) = value else {
        return Err(format!(
            "an '{op}' operation without a path needs an object as its value"
        ));
    };
    for (name, value) in attributes {
        match value {
            serde_json::Value::Object(extension)
                if name.to_ascii_lowercase().starts_with("urn:") =>
            {
                for (attribute, value) in extension {
                    operations.push(Operation {
                        op: op.clone(),
                        path: format!("{name}:{attribute}"),
                        value: Some(value),
                    });
                }
            }
            value => operations.push(Operation {
                op: op.clone(),
                path: name,
                value: Some(value),
            }),
        }
    }
    Ok(())
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
    use super::{PatchOp, normalize_attr};
    use serde_json::json;

    fn parse(operations: serde_json::Value) -> Result<PatchOp, serde_json::Error> {
        serde_json::from_value(json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": operations,
        }))
    }

    #[test]
    fn a_replace_without_a_path_becomes_one_operation_per_attribute() {
        let urn = "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User";
        let patch = parse(json!([{
            "op": "replace",
            "value": { "active": false, "displayName": "Jane", (urn): { "department": "R&D" } },
        }]))
        .unwrap();

        let ops: Vec<_> = patch
            .operations
            .iter()
            .map(|o| (o.op.as_str(), o.path.as_str(), o.value.clone().unwrap()))
            .collect();
        assert_eq!(
            ops,
            [
                ("replace", "active", json!(false)),
                ("replace", "displayName", json!("Jane")),
                ("replace", &*format!("{urn}:department"), json!("R&D")),
            ]
        );
        assert_eq!(normalize_attr(&patch.operations[2].path), "department");
    }

    #[test]
    fn an_operation_without_a_path_is_refused_unless_it_can_be_expanded() {
        assert!(parse(json!([{ "op": "remove" }])).is_err());
        assert!(parse(json!([{ "op": "replace", "value": false }])).is_err());
    }

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
