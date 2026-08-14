use std::collections::BTreeMap;
use std::sync::Mutex;

use actix_web::http::{Method, StatusCode, header};
use actix_web::test as actix_test;
use uuid::Uuid;

use crate::models::{FilterQuery, ListResponse, Meta, PatchOp, Schema, User, ValOrEmpty};
use crate::resource::finalize;
use crate::{AppBuilder, Error, Resource, ResourceService, Result, configure};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct Thing {
    schemas: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Uuid>,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<Meta>,
}

impl Thing {
    fn new(display_name: &str) -> Self {
        Self {
            schemas: vec![Self::SCHEMA.to_string()],
            id: None,
            display_name: display_name.to_string(),
            meta: None,
        }
    }
}

impl Resource for Thing {
    type Id = Uuid;

    const RESOURCE_TYPE: &'static str = "Thing";
    const ENDPOINT: &'static str = "/Things";
    const SCHEMA: &'static str = "urn:example:params:scim:schemas:Thing";

    fn id(&self) -> Option<&Self::Id> {
        self.id.as_ref()
    }

    fn meta(&self) -> Option<&Meta> {
        self.meta.as_ref()
    }

    fn set_meta(&mut self, meta: Meta) {
        self.meta = Some(meta);
    }

    fn schema() -> Schema {
        Schema::new(
            Self::SCHEMA,
            "Thing",
            "A thing",
            vec![crate::models::Attributes::new("displayName", "string").required()],
        )
    }
}

#[derive(Default)]
struct ThingStore {
    things: Mutex<BTreeMap<Uuid, Thing>>,
}

impl ThingStore {
    fn things(&self) -> std::sync::MutexGuard<'_, BTreeMap<Uuid, Thing>> {
        self.things.lock().expect("store is not poisoned")
    }
}

#[crate::async_trait]
impl ResourceService<Thing> for ThingStore {
    async fn list(&self, _filter: FilterQuery) -> Result<ListResponse<Thing>> {
        Ok(ListResponse::new(self.things().values().cloned().collect()))
    }

    async fn create(&self, mut thing: Thing) -> Result<Thing> {
        let id = thing.id.unwrap_or_else(Uuid::new_v4);
        thing.id = Some(id);
        // Deliberately sets only `created`, to prove `finalize` fills the rest without clobbering
        // what the service supplied.
        thing.meta = Some(Meta {
            created: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        });
        self.things().insert(id, thing.clone());
        Ok(thing)
    }

    async fn get(&self, id: Uuid) -> Result<Thing> {
        self.things()
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }

    async fn replace(&self, id: Uuid, mut thing: Thing) -> Result<Thing> {
        let mut things = self.things();
        if !things.contains_key(&id) {
            return Err(Error::NotFound(id.to_string()));
        }
        thing.id = Some(id);
        things.insert(id, thing.clone());
        Ok(thing)
    }

    async fn patch(&self, id: Uuid, op: PatchOp) -> Result<ValOrEmpty<Thing>> {
        let mut things = self.things();
        let thing = things
            .get_mut(&id)
            .ok_or_else(|| Error::NotFound(id.to_string()))?;
        match op.operations.first() {
            None => Ok(ValOrEmpty::Empty),
            Some(operation) => {
                thing.display_name = operation
                    .value
                    .as_ref()
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                Ok(ValOrEmpty::Val(thing.clone()))
            }
        }
    }

    async fn delete(&self, id: Uuid) -> Result<()> {
        self.things()
            .remove(&id)
            .map(|_| ())
            .ok_or_else(|| Error::NotFound(id.to_string()))
    }
}

struct SilentService {}

#[crate::async_trait]
impl ResourceService<Thing> for SilentService {}

macro_rules! test_app {
    ($builder:expr) => {{
        let state = $builder.build().state();
        actix_test::init_service(actix_web::App::new().configure(move |cfg| configure(cfg, state)))
            .await
    }};
}

fn request(method: Method, uri: &str) -> actix_test::TestRequest {
    actix_test::TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((header::ACCEPT, "application/scim+json"))
}

#[actix_web::test]
async fn crud_round_trip_through_dynamic_routes() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let created: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Things")
            .set_json(Thing::new("first"))
            .to_request(),
    )
    .await;
    let id = created["id"].as_str().expect("created thing has an id");
    assert_eq!(created["displayName"], "first");

    let fetched: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::GET, &format!("/v2/Things/{id}")).to_request(),
    )
    .await;
    assert_eq!(fetched["displayName"], "first");

    let listed: serde_json::Value =
        actix_test::call_and_read_body_json(&app, request(Method::GET, "/v2/Things").to_request())
            .await;
    assert_eq!(listed["totalResults"], 1);
    assert_eq!(listed["Resources"][0]["id"], id);

    let replaced: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::PUT, &format!("/v2/Things/{id}"))
            .set_json(Thing::new("second"))
            .to_request(),
    )
    .await;
    assert_eq!(replaced["displayName"], "second");
    assert_eq!(replaced["id"], id, "PUT must not reassign the identifier");

    let deleted = actix_test::call_service(
        &app,
        request(Method::DELETE, &format!("/v2/Things/{id}")).to_request(),
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let gone = actix_test::call_service(
        &app,
        request(Method::GET, &format!("/v2/Things/{id}")).to_request(),
    )
    .await;
    assert_eq!(gone.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn unimplemented_operations_answer_501() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(SilentService {}));

    for (method, uri) in [
        (Method::GET, "/v2/Things"),
        (Method::POST, "/v2/Things"),
        (
            Method::GET,
            "/v2/Things/f7b3c0de-0000-4000-8000-000000000000",
        ),
        (
            Method::PUT,
            "/v2/Things/f7b3c0de-0000-4000-8000-000000000000",
        ),
        (
            Method::PATCH,
            "/v2/Things/f7b3c0de-0000-4000-8000-000000000000",
        ),
        (
            Method::DELETE,
            "/v2/Things/f7b3c0de-0000-4000-8000-000000000000",
        ),
    ] {
        let req = match method {
            Method::POST | Method::PUT => request(method.clone(), uri).set_json(Thing::new("x")),
            Method::PATCH => request(method.clone(), uri).set_json(PatchOp::default()),
            _ => request(method.clone(), uri),
        };
        let resp = actix_test::call_service(&app, req.to_request()).await;
        assert_eq!(
            resp.status(),
            StatusCode::NOT_IMPLEMENTED,
            "{method} {uri} should be 501"
        );
    }
}

#[actix_web::test]
async fn unregistered_endpoint_is_404() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let resp =
        actix_test::call_service(&app, request(Method::GET, "/v2/Widgets").to_request()).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn scim_accept_header_is_required() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let resp = actix_test::call_service(
        &app,
        actix_test::TestRequest::default()
            .uri("/v2/Things")
            .insert_header((header::ACCEPT, "application/json"))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[actix_web::test]
async fn bearer_token_is_enforced() {
    let app = test_app!(
        AppBuilder::new()
            .register::<Thing, _>(ThingStore::default())
            .bearer_token(Some("s3cret".into()))
    );

    let unauthorized =
        actix_test::call_service(&app, request(Method::GET, "/v2/Things").to_request()).await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let wrong_token = actix_test::call_service(
        &app,
        request(Method::GET, "/v2/Things")
            .insert_header((header::AUTHORIZATION, "Bearer nope"))
            .to_request(),
    )
    .await;
    assert_eq!(wrong_token.status(), StatusCode::UNAUTHORIZED);

    let authorized = actix_test::call_service(
        &app,
        request(Method::GET, "/v2/Things")
            .insert_header((header::AUTHORIZATION, "Bearer s3cret"))
            .to_request(),
    )
    .await;
    assert_eq!(authorized.status(), StatusCode::OK);
}

#[actix_web::test]
async fn unparseable_id_is_a_scim_bad_request() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let resp = actix_test::call_service(
        &app,
        request(Method::GET, "/v2/Things/not-a-uuid").to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body: serde_json::Value = actix_test::read_body_json(resp).await;
    assert_eq!(
        body["schemas"][0],
        "urn:ietf:params:scim:api:messages:2.0:Error"
    );
    assert_eq!(body["status"], "400");
    assert!(
        body["detail"]
            .as_str()
            .is_some_and(|d| d.contains("not a valid Thing id")),
        "unhelpful detail: {}",
        body["detail"]
    );
}

#[actix_web::test]
async fn malformed_payload_is_a_scim_bad_request() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let resp = actix_test::call_service(
        &app,
        request(Method::POST, "/v2/Things")
            .set_json(serde_json::json!({"schemas": [Thing::SCHEMA]}))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body: serde_json::Value = actix_test::read_body_json(resp).await;
    assert_eq!(body["status"], "400");
}

#[actix_web::test]
async fn patch_maps_val_to_200_and_empty_to_204() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let created: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Things")
            .set_json(Thing::new("before"))
            .to_request(),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_owned();

    let empty = actix_test::call_service(
        &app,
        request(Method::PATCH, &format!("/v2/Things/{id}"))
            .set_json(PatchOp::default())
            .to_request(),
    )
    .await;
    assert_eq!(empty.status(), StatusCode::NO_CONTENT);

    let mut op = PatchOp::default();
    op.operations
        .push(crate::models::patchop::Operation::replace(
            "displayName".into(),
            Some(serde_json::json!("after")),
        ));
    let patched: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::PATCH, &format!("/v2/Things/{id}"))
            .set_json(op)
            .to_request(),
    )
    .await;
    assert_eq!(patched["displayName"], "after");
}

#[actix_web::test]
async fn discovery_is_derived_from_the_registry() {
    let app = test_app!(
        AppBuilder::new()
            .register::<User, _>(SilentUserService {})
            .register::<Thing, _>(ThingStore::default())
    );

    let types: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::GET, "/v2/ResourceTypes").to_request(),
    )
    .await;
    assert_eq!(types["totalResults"], 2);
    assert_eq!(types["Resources"][0]["id"], "Thing");
    assert_eq!(types["Resources"][0]["endpoint"], "/Things");
    assert_eq!(types["Resources"][1]["id"], "User");
    assert_eq!(types["Resources"][1]["schema"], User::SCHEMA);

    let one: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::GET, "/v2/ResourceTypes/User").to_request(),
    )
    .await;
    assert_eq!(one["endpoint"], "/Users");
    assert_eq!(one["meta"]["location"], "/v2/ResourceTypes/User");

    let missing = actix_test::call_service(
        &app,
        request(Method::GET, "/v2/ResourceTypes/Nope").to_request(),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let schemas: serde_json::Value =
        actix_test::call_and_read_body_json(&app, request(Method::GET, "/v2/Schemas").to_request())
            .await;
    assert_eq!(schemas["totalResults"], 2);

    let user_schema: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::GET, &format!("/v2/Schemas/{}", User::SCHEMA)).to_request(),
    )
    .await;
    assert_eq!(user_schema["id"], User::SCHEMA);
    assert_eq!(user_schema["attributes"][0]["name"], "userName");

    let missing_schema = actix_test::call_service(
        &app,
        request(Method::GET, "/v2/Schemas/urn:example:nope").to_request(),
    )
    .await;
    assert_eq!(missing_schema.status(), StatusCode::NOT_FOUND);
}

struct SilentUserService {}

#[crate::async_trait]
impl ResourceService<User> for SilentUserService {}

#[test]
fn builtin_schemas_are_well_formed() {
    for (schema, urn, attribute_names) in [
        (
            User::schema(),
            User::SCHEMA,
            // RFC 7643 section 8.7.1, in order.
            &[
                "userName",
                "name",
                "displayName",
                "nickName",
                "profileUrl",
                "title",
                "userType",
                "preferredLanguage",
                "locale",
                "timezone",
                "active",
                "password",
                "emails",
                "phoneNumbers",
                "ims",
                "photos",
                "addresses",
                "groups",
                "entitlements",
                "roles",
                "x509Certificates",
            ][..],
        ),
        (
            crate::models::Group::schema(),
            crate::models::Group::SCHEMA,
            &["displayName", "members"][..],
        ),
    ] {
        assert_eq!(schema.id, urn);
        assert_eq!(schema.meta.resource_type.as_deref(), Some("Schema"));
        assert_eq!(
            schema.meta.location.as_deref(),
            Some(format!("/v2/Schemas/{urn}").as_str())
        );
        assert_eq!(
            schema
                .attributes
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            attribute_names
        );

        for attribute in &schema.attributes {
            if attribute.type_ == "complex" {
                assert!(
                    attribute
                        .sub_attributes
                        .as_ref()
                        .is_some_and(|s| !s.is_empty()),
                    "{urn}: complex attribute {} has no subAttributes",
                    attribute.name
                );
            }
            if matches!(attribute.type_.as_str(), "complex" | "boolean") {
                assert_eq!(
                    (attribute.case_exact, attribute.uniqueness.as_deref()),
                    (None, None),
                    "{urn}: {} is not string-like but carries string-only qualifiers",
                    attribute.name
                );
            }
        }
    }
}

#[test]
fn finalize_fills_only_the_meta_the_service_left_out() {
    let id = Uuid::new_v4();

    let mut thing = Thing {
        id: Some(id),
        ..Thing::new("bare")
    };
    finalize(&mut thing);
    let meta = thing.meta.expect("meta was filled in");
    assert_eq!(meta.resource_type.as_deref(), Some("Thing"));
    assert_eq!(meta.location, Some(format!("/v2/Things/{id}")));

    let mut thing = Thing {
        id: Some(id),
        meta: Some(Meta {
            created: Some("2026-01-01T00:00:00Z".into()),
            location: Some("https://example.test/custom".into()),
            ..Default::default()
        }),
        ..Thing::new("opinionated")
    };
    finalize(&mut thing);
    let meta = thing.meta.expect("meta survived");
    assert_eq!(
        meta.location.as_deref(),
        Some("https://example.test/custom"),
        "a location set by the service must not be overwritten"
    );
    assert_eq!(meta.created.as_deref(), Some("2026-01-01T00:00:00Z"));
    assert_eq!(meta.resource_type.as_deref(), Some("Thing"));

    let mut thing = Thing::new("unsaved");
    finalize(&mut thing);
    assert_eq!(thing.meta.expect("meta was filled in").location, None);
}

#[actix_web::test]
async fn create_response_carries_generated_meta() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let created: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Things")
            .set_json(Thing::new("first"))
            .to_request(),
    )
    .await;

    let id = created["id"].as_str().unwrap();
    assert_eq!(created["meta"]["resourceType"], "Thing");
    assert_eq!(created["meta"]["location"], format!("/v2/Things/{id}"));
    assert_eq!(
        created["meta"]["created"], "2026-01-01T00:00:00Z",
        "the value the service set must survive"
    );
}

#[actix_web::test]
async fn response_keys_keep_declaration_order() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let body = actix_test::call_and_read_body(
        &app,
        request(Method::POST, "/v2/Things")
            .set_json(Thing::new("first"))
            .to_request(),
    )
    .await;
    let body = std::str::from_utf8(&body).expect("response is UTF-8");

    let positions: Vec<usize> = ["\"schemas\"", "\"id\"", "\"displayName\"", "\"meta\""]
        .iter()
        .map(|key| {
            body.find(key)
                .unwrap_or_else(|| panic!("{key} missing from {body}"))
        })
        .collect();
    assert!(
        positions.is_sorted(),
        "keys are not in declaration order: {body}"
    );
}
