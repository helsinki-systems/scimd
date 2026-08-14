//! The `/Bulk` executor (RFC 7644 section 3.7).
//!
//! Bulk is dispatch over the handler registry, so it drives exactly the same
//! [`crate::resource::ErasedHandler`] methods the individual routes do and needs nothing new from
//! them. What lives here is the batch's own state: the `bulkId` map, the failure count, and the
//! per-operation result documents.

use std::collections::HashMap;

use actix_web::ResponseError as _;
use actix_web::http::StatusCode;
use serde_json::Value;

use crate::models::{BulkOperation, BulkOperationResult, BulkRequest, BulkResponse};
use crate::{Error, Result, State};

/// `bulkId` references appear as `bulkId:<key>`, either as a whole value or embedded in a `$ref`
/// such as `../Users/bulkId:qwerty`.
const BULK_ID_PREFIX: &str = "bulkId:";

/// Splits an operation path into the endpoint and, if present, the resource id.
///
/// `/Users` -> `("/Users", None)`, `/Users/2819c223` -> `("/Users", Some("2819c223"))`. Anything
/// deeper is not a resource path and yields `None`.
fn split_path(path: &str) -> Option<(&str, Option<&str>)> {
    let rest = path.strip_prefix('/')?;
    match rest.split_once('/') {
        None if rest.is_empty() => None,
        None => Some((path, None)),
        Some((endpoint, id)) if !endpoint.is_empty() && !id.is_empty() && !id.contains('/') => {
            Some((&path[..endpoint.len() + 1], Some(id)))
        }
        Some(_) => None,
    }
}

/// Where a `bulkId:` key ends: at a path separator, a quote, or the end of the string.
fn resolve_references(text: &str, ids: &HashMap<String, String>) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find(BULK_ID_PREFIX) {
        out.push_str(&rest[..at]);
        let after = &rest[at + BULK_ID_PREFIX.len()..];
        let end = after.find(['/', '"']).unwrap_or(after.len());
        let key = &after[..end];

        let id = ids.get(key).ok_or_else(|| {
            Error::InvalidValue(format!(
                "bulkId '{key}' has not been assigned by an earlier operation"
            ))
        })?;
        out.push_str(id);
        rest = &after[end..];
    }

    out.push_str(rest);
    Ok(out)
}

/// Rewrites every `bulkId:` reference in a payload, wherever it is nested.
fn resolve_in_value(value: &mut Value, ids: &HashMap<String, String>) -> Result<()> {
    match value {
        Value::String(s) => {
            if s.contains(BULK_ID_PREFIX) {
                *s = resolve_references(s, ids)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                resolve_in_value(item, ids)?;
            }
        }
        Value::Object(fields) => {
            for (_, field) in fields.iter_mut() {
                resolve_in_value(field, ids)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// What one operation produced when it succeeded: the status to report, where the resource lives,
/// and its id if it created one.
struct Applied {
    status: StatusCode,
    location: Option<String>,
    id: Option<String>,
}

fn location_of(resource: &Value) -> Option<String> {
    resource
        .get("meta")?
        .get("location")?
        .as_str()
        .map(str::to_string)
}

fn id_of(resource: &Value) -> Option<String> {
    match resource.get("id")? {
        Value::String(id) => Some(id.clone()),
        // A resource is free to serialize its id as something other than a string.
        other if !other.is_null() => Some(other.to_string()),
        _ => None,
    }
}

async fn apply(state: &State, operation: &BulkOperation, data: Option<Value>) -> Result<Applied> {
    let (endpoint, id) = split_path(&operation.path)
        .ok_or_else(|| Error::BadRequest(format!("'{}' is not a resource path", operation.path)))?;
    let handler = state
        .handlers
        .get(endpoint)
        .cloned()
        .ok_or_else(|| Error::NotFound(operation.path.clone()))?;

    let body = || {
        data.clone().ok_or_else(|| {
            Error::BadRequest(format!(
                "{} {} requires data",
                operation.method, operation.path
            ))
        })
    };

    match (operation.method.to_ascii_uppercase().as_str(), id) {
        ("POST", None) => {
            let created = handler.create(body()?).await?;
            Ok(Applied {
                status: StatusCode::CREATED,
                location: location_of(&created),
                id: id_of(&created),
            })
        }
        ("PUT", Some(id)) => {
            let replaced = handler.replace(id, body()?).await?;
            Ok(Applied {
                status: StatusCode::OK,
                location: location_of(&replaced),
                id: None,
            })
        }
        ("PATCH", Some(id)) => {
            let op = serde_json::from_value(body()?)
                .map_err(|e| Error::BadRequest(format!("invalid PatchOp: {e}")))?;
            match handler.patch(id, op).await? {
                Some(patched) => Ok(Applied {
                    status: StatusCode::OK,
                    location: location_of(&patched),
                    id: None,
                }),
                None => Ok(Applied {
                    status: StatusCode::NO_CONTENT,
                    location: None,
                    id: None,
                }),
            }
        }
        ("DELETE", Some(id)) => {
            handler.delete(id).await?;
            Ok(Applied {
                status: StatusCode::NO_CONTENT,
                location: None,
                id: None,
            })
        }
        (method, _) => Err(Error::BadRequest(format!(
            "{method} is not valid against '{}'",
            operation.path
        ))),
    }
}

/// Runs a batch. Operations are applied in the order given and a failing one never aborts the
/// rest - it is reported with its own status and error document - unless `failOnErrors` says
/// otherwise.
pub(crate) async fn execute(state: &State, request: BulkRequest) -> Result<BulkResponse> {
    if let Some(max) = state.bulk_settings.max_operations
        && request.operations.len() > max as usize
    {
        return Err(Error::PayloadTooLarge(format!(
            "the request has {} operations, more than the {max} supported",
            request.operations.len()
        )));
    }

    // bulkId -> the id the POST actually got. Sequential: an operation can only reference a
    // bulkId defined by an operation before it.
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut results = Vec::with_capacity(request.operations.len());
    let mut failures = 0;

    for operation in &request.operations {
        let outcome = resolve_operation(state, operation, &ids).await;

        let result = BulkOperationResult::new(
            &operation.method,
            operation.bulk_id.clone(),
            match &outcome {
                Ok(applied) => applied.status,
                Err(e) => e.status_code(),
            },
        );

        match outcome {
            Ok(applied) => {
                if let (Some(bulk_id), Some(id)) = (&operation.bulk_id, applied.id) {
                    ids.insert(bulk_id.clone(), id);
                }
                results.push(result.with_location(applied.location));
            }
            Err(e) => {
                tracing::debug!(
                    "bulk operation {} {} failed: {e}",
                    operation.method,
                    operation.path
                );
                failures += 1;
                results.push(
                    result.with_response(
                        serde_json::to_value(e.to_scim_error())
                            .map_err(|e| Error::InternalError(e.into()))?,
                    ),
                );
                if request.fail_on_errors.is_some_and(|max| failures >= max) {
                    break;
                }
            }
        }
    }

    Ok(BulkResponse::new(results))
}

/// Substitution has to happen per operation rather than up front, because the map only fills in as
/// earlier operations complete.
async fn resolve_operation(
    state: &State,
    operation: &BulkOperation,
    ids: &HashMap<String, String>,
) -> Result<Applied> {
    let mut resolved = BulkOperation {
        method: operation.method.clone(),
        path: resolve_references(&operation.path, ids)?,
        bulk_id: operation.bulk_id.clone(),
        version: operation.version.clone(),
        data: None,
    };

    let data = match operation.data.clone() {
        Some(mut data) => {
            resolve_in_value(&mut data, ids)?;
            Some(data)
        }
        None => None,
    };
    resolved.data = data.clone();

    apply(state, &resolved, data).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_resource_paths() {
        assert_eq!(split_path("/Users"), Some(("/Users", None)));
        assert_eq!(split_path("/Users/abc"), Some(("/Users", Some("abc"))));
        assert_eq!(split_path("/Users/abc/extra"), None);
        assert_eq!(split_path("/Users/"), None);
        assert_eq!(split_path("Users"), None);
        assert_eq!(split_path("/"), None);
        assert_eq!(split_path(""), None);
    }

    #[test]
    fn resolves_references_in_every_position() {
        let ids = HashMap::from([("qwerty".to_string(), "real-id".to_string())]);

        assert_eq!(
            resolve_references("bulkId:qwerty", &ids).unwrap(),
            "real-id"
        );
        assert_eq!(
            resolve_references("../Users/bulkId:qwerty", &ids).unwrap(),
            "../Users/real-id"
        );
        assert_eq!(
            resolve_references("/Users/bulkId:qwerty", &ids).unwrap(),
            "/Users/real-id"
        );
        assert_eq!(
            resolve_references("nothing here", &ids).unwrap(),
            "nothing here"
        );
        assert!(resolve_references("bulkId:unknown", &ids).is_err());
    }

    #[test]
    fn resolves_references_nested_in_data() {
        let ids = HashMap::from([("qwerty".to_string(), "real-id".to_string())]);
        let mut data = serde_json::json!({
            "displayName": "Tour Guides",
            "members": [{"type": "User", "value": "bulkId:qwerty"}]
        });

        resolve_in_value(&mut data, &ids).unwrap();
        assert_eq!(data["members"][0]["value"], "real-id");
        assert_eq!(data["displayName"], "Tour Guides");
    }
}
