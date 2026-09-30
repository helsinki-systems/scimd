use super::user::{Email, primary_email};

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

    // Operation names are case-insensitive in practice: Entra ID capitalises them.

    pub fn is_add(&self) -> bool {
        self.op.eq_ignore_ascii_case("add")
    }

    pub fn is_remove(&self) -> bool {
        self.op.eq_ignore_ascii_case("remove")
    }

    pub fn is_replace(&self) -> bool {
        self.op.eq_ignore_ascii_case("replace")
    }

    /// The attribute this operation targets, lowercased and with the URN prefix, any `valuePath`
    /// filter and any sub-attribute stripped: `urn:...:User:emails[type eq "work"].value` is an
    /// operation on `emails`. See [`normalize_attr`] for why the URN goes.
    pub fn attr(&self) -> String {
        // The `valuePath` goes first, since its filter may quote a colon or a dot.
        let path = self.path.split('[').next().unwrap_or_default();
        let attr = normalize_attr(path);
        attr.split('.').next().unwrap_or_default().to_owned()
    }

    pub fn str_value(&self) -> Option<&str> {
        self.value.as_ref()?.as_str()
    }

    /// A boolean, which some clients send as a JSON string rather than a JSON boolean.
    pub fn bool_value(&self) -> Option<bool> {
        match self.value.as_ref()? {
            serde_json::Value::Bool(b) => Some(*b),
            serde_json::Value::String(s) if s.eq_ignore_ascii_case("true") => Some(true),
            serde_json::Value::String(s) if s.eq_ignore_ascii_case("false") => Some(false),
            _ => None,
        }
    }

    /// The address an `emails` operation carries, written either as a bare string under
    /// `emails[type eq "work"].value` or under `emails` as the multi-valued attribute or a single
    /// element of it. Of several addresses, the one [`User::primary_email`] would pick wins.
    ///
    /// [`User::primary_email`]: super::User::primary_email
    pub fn email_address(&self) -> Option<String> {
        use serde::Deserialize as _;

        let value = self.value.as_ref()?;
        if let serde_json::Value::String(address) = value {
            return Some(address.clone());
        }
        // Deserialising from `&Value` rather than `Value` keeps both attempts clone-free.
        let emails = <Vec<Email>>::deserialize(value)
            .or_else(|_| Email::deserialize(value).map(|one| vec![one]))
            .ok()?;
        primary_email(&emails).map(ToOwned::to_owned)
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
    use super::{Operation, PatchOp, normalize_attr};
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

    fn op(name: &str, path: &str, value: serde_json::Value) -> Operation {
        Operation {
            op: name.into(),
            path: path.into(),
            value: Some(value),
        }
    }

    #[test]
    fn an_operation_name_is_read_case_insensitively() {
        assert!(op("Replace", "active", json!(false)).is_replace());
        assert!(Operation::remove("members".into()).is_remove());
        assert!(!op("add", "members", json!([])).is_remove());
    }

    #[test]
    fn a_patch_path_reduces_to_the_attribute_it_targets() {
        let attr = |path: &str| Operation::remove(path.into()).attr();
        for path in [
            "emails",
            "EMAILS",
            "emails.value",
            "emails[type eq \"work\"].value",
            "urn:ietf:params:scim:schemas:core:2.0:User:emails",
            "urn:ietf:params:scim:schemas:core:2.0:User:emails[value ew \"a.b:c\"]",
        ] {
            assert_eq!(attr(path), "emails", "for '{path}'");
        }
    }

    #[test]
    fn a_boolean_is_read_whether_it_arrives_as_one_or_as_a_string() {
        assert_eq!(
            op("replace", "active", json!(false)).bool_value(),
            Some(false)
        );
        assert_eq!(
            op("replace", "active", json!("False")).bool_value(),
            Some(false),
            "some identity providers quote their booleans"
        );
        assert_eq!(op("replace", "active", json!("yes")).bool_value(), None);
    }

    #[test]
    fn an_email_patch_is_read_from_every_shape_it_arrives_in() {
        let shapes = [
            op(
                "replace",
                "emails[type eq \"work\"].value",
                json!("new@b.c"),
            ),
            op(
                "replace",
                "emails",
                json!([{"value": "old@b.c"}, {"value": "new@b.c", "primary": true}]),
            ),
            op("replace", "emails", json!({"value": "new@b.c"})),
        ];
        for operation in shapes {
            assert_eq!(
                operation.email_address().as_deref(),
                Some("new@b.c"),
                "{operation:?}"
            );
        }
    }
}
