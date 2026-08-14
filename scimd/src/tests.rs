use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use actix_web::http::{Method, StatusCode, header};
use actix_web::test as actix_test;
use uuid::Uuid;

use crate::models::{
    FilterQuery, ListResponse, Meta, PatchOp, Schema, User, ValOrEmpty, enterprise_user,
};
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
    seen: Arc<Mutex<Option<FilterQuery>>>,
}

impl ThingStore {
    fn things(&self) -> std::sync::MutexGuard<'_, BTreeMap<Uuid, Thing>> {
        self.things.lock().expect("store is not poisoned")
    }
}

#[crate::async_trait]
impl ResourceService<Thing> for ThingStore {
    async fn list(&self, filter: FilterQuery) -> Result<ListResponse<Thing>> {
        let things: Vec<Thing> = self.things().values().cloned().collect();
        let total = things.len();
        let paginate = filter.start_index.is_some() || filter.count.is_some();
        let start_index = filter.start_index.unwrap_or(1);
        *self.seen.lock().expect("store is not poisoned") = Some(filter);

        if paginate {
            let page = things
                .into_iter()
                .skip(start_index.saturating_sub(1))
                .collect();
            Ok(ListResponse::paginated(page, total, start_index))
        } else {
            Ok(ListResponse::new(things))
        }
    }

    async fn create(&self, mut thing: Thing) -> Result<Thing> {
        let id = thing.id.unwrap_or_else(Uuid::new_v4);
        thing.id = Some(id);
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
        let state = $builder.build().state(None);
        actix_test::init_service(actix_web::App::new().configure(move |cfg| configure(cfg, state)))
            .await
    }};
}

macro_rules! full_app {
    ($builder:expr, $metrics_enabled:expr) => {{
        let app = $builder.with_metrics($metrics_enabled).build();
        let metrics = app.metrics().expect("the metrics register cleanly");
        let state = app.state($metrics_enabled.then(|| metrics.clone()));
        let service =
            actix_test::init_service(scim_app!(state, $metrics_enabled, metrics.middleware()))
                .await;
        (service, metrics)
    }};
}

macro_rules! metrics_app {
    ($builder:expr) => {
        full_app!($builder, true)
    };
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
async fn acceptable_media_types_are_served() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    for accept in [
        None,
        Some(""),
        Some("*/*"),
        Some("application/*"),
        Some("application/scim+json"),
        Some("application/json"),
        Some("APPLICATION/SCIM+JSON"),
        Some("application/scim+json; charset=utf-8"),
        Some("text/html, application/xhtml+xml, application/scim+json;q=0.1"),
        Some("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    ] {
        let mut req = actix_test::TestRequest::default().uri("/v2/Things");
        if let Some(accept) = accept {
            req = req.insert_header((header::ACCEPT, accept));
        }

        let resp = actix_test::call_service(&app, req.to_request()).await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Accept: {accept:?} should have been served"
        );
    }
}

#[actix_web::test]
async fn unacceptable_media_types_are_406_not_404() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    for accept in ["text/html", "application/xml", "text/plain, image/png"] {
        let resp = actix_test::call_service(
            &app,
            actix_test::TestRequest::default()
                .uri("/v2/Things")
                .insert_header((header::ACCEPT, accept))
                .to_request(),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::NOT_ACCEPTABLE,
            "Accept: {accept} should have been refused"
        );

        let body: serde_json::Value = actix_test::read_body_json(resp).await;
        assert_eq!(
            body["schemas"][0],
            "urn:ietf:params:scim:api:messages:2.0:Error"
        );
        assert_eq!(body["status"], "406");
    }
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
    assert_eq!(schemas["totalResults"], 3);
    assert_eq!(
        schemas["Resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![Thing::SCHEMA, User::SCHEMA, enterprise_user::SCHEMA]
    );

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

struct OneUserService {
    user: Mutex<Option<User>>,
}

#[crate::async_trait]
impl ResourceService<User> for OneUserService {
    async fn get(&self, _id: String) -> Result<User> {
        self.user
            .lock()
            .expect("store is not poisoned")
            .take()
            .ok_or_else(|| Error::NotFound("already taken".into()))
    }
}

#[actix_web::test]
async fn extensions_are_advertised_in_resource_types() {
    let app = test_app!(AppBuilder::new().register::<User, _>(SilentUserService {}));

    let user: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::GET, "/v2/ResourceTypes/User").to_request(),
    )
    .await;
    assert_eq!(
        user["schemaExtensions"][0]["schema"],
        enterprise_user::SCHEMA
    );
    assert_eq!(user["schemaExtensions"][0]["required"], false);

    let extension: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(
            Method::GET,
            &format!("/v2/Schemas/{}", enterprise_user::SCHEMA),
        )
        .to_request(),
    )
    .await;
    assert_eq!(extension["id"], enterprise_user::SCHEMA);
    assert_eq!(extension["name"], "EnterpriseUser");
    assert_eq!(extension["attributes"][0]["name"], "employeeNumber");

    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));
    let thing: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::GET, "/v2/ResourceTypes/Thing").to_request(),
    )
    .await;
    assert!(thing.get("schemaExtensions").is_none());
}

#[actix_web::test]
async fn present_extension_data_is_declared_in_schemas() {
    let carrying = User {
        id: Some("07756fc0-0000-4000-8000-000000000000".into()),
        enterprise_user: Some(enterprise_user::EnterpriseUser {
            department: Some("Software".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let app = test_app!(AppBuilder::new().register::<User, _>(OneUserService {
        user: Mutex::new(Some(carrying))
    }));

    let user: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(
            Method::GET,
            "/v2/Users/07756fc0-0000-4000-8000-000000000000",
        )
        .to_request(),
    )
    .await;
    assert_eq!(
        user["schemas"],
        serde_json::json!([User::SCHEMA, enterprise_user::SCHEMA]),
        "the extension is present, so its URN must be declared"
    );
    assert_eq!(user[enterprise_user::SCHEMA]["department"], "Software");
}

#[actix_web::test]
async fn absent_extension_data_is_not_declared() {
    let bare = User {
        id: Some("07756fc0-0000-4000-8000-000000000001".into()),
        ..Default::default()
    };
    let app = test_app!(AppBuilder::new().register::<User, _>(OneUserService {
        user: Mutex::new(Some(bare))
    }));

    let user: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(
            Method::GET,
            "/v2/Users/07756fc0-0000-4000-8000-000000000001",
        )
        .to_request(),
    )
    .await;
    assert_eq!(
        user["schemas"],
        serde_json::json!([User::SCHEMA]),
        "an extension that is not carried must not be announced"
    );
}

#[test]
fn enterprise_user_is_namespaced_under_its_urn() {
    let user = User {
        enterprise_user: Some(enterprise_user::EnterpriseUser::default()),
        ..Default::default()
    };
    let value = serde_json::to_value(&user).unwrap();
    assert!(
        value.get(enterprise_user::SCHEMA).is_some(),
        "User serializes its extension under a different key than {}",
        enterprise_user::SCHEMA
    );
}

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

fn search_body(filter: Option<&str>) -> crate::models::SearchRequest {
    crate::models::SearchRequest {
        filter: filter.map(str::to_string),
        ..Default::default()
    }
}

macro_rules! seed {
    ($app:expr, $name:expr) => {{
        let created: serde_json::Value = actix_test::call_and_read_body_json(
            &$app,
            request(Method::POST, "/v2/Things")
                .set_json(Thing::new($name))
                .to_request(),
        )
        .await;
        created["id"]
            .as_str()
            .expect("created thing has an id")
            .to_owned()
    }};
}

#[actix_web::test]
async fn create_answers_201_with_a_location() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let resp = actix_test::call_service(
        &app,
        request(Method::POST, "/v2/Things")
            .set_json(Thing::new("first"))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::CREATED);
    let location = resp
        .headers()
        .get(header::LOCATION)
        .expect("a created resource says where it lives")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        location.starts_with("/v2/Things/"),
        "unexpected Location: {location}"
    );
}

#[actix_web::test]
async fn search_reaches_the_same_list_as_get() {
    let store = ThingStore::default();
    let seen = store.seen.clone();
    let app = test_app!(AppBuilder::new().register::<Thing, _>(store));
    let _ = seed!(app, "first");

    let searched: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Things/.search")
            .set_json(search_body(Some("displayName eq \"first\"")))
            .to_request(),
    )
    .await;

    assert_eq!(searched["totalResults"], 1);
    assert_eq!(searched["Resources"][0]["displayName"], "first");
    assert_eq!(
        seen.lock().unwrap().as_ref().unwrap().filter.as_deref(),
        Some("displayName eq \"first\""),
        "the filter from the body must reach the service"
    );
}

#[actix_web::test]
async fn search_route_is_not_swallowed_by_the_id_route() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let resp = actix_test::call_service(
        &app,
        request(Method::POST, "/v2/Things/.search")
            .set_json(search_body(None))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    let body: serde_json::Value = actix_test::read_body_json(resp).await;
    assert_eq!(
        body["schemas"][0], "urn:ietf:params:scim:api:messages:2.0:ListResponse",
        "a search must answer with a list, not with a resource named '.search'"
    );
}

#[actix_web::test]
async fn root_search_fans_out_across_resource_types() {
    let app = test_app!(
        AppBuilder::new()
            .register::<Thing, _>(ThingStore::default())
            .register::<User, _>(SilentUserService {})
    );
    let _ = seed!(app, "first");
    let _ = seed!(app, "second");

    let searched: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/.search")
            .set_json(search_body(None))
            .to_request(),
    )
    .await;

    assert_eq!(searched["totalResults"], 2);
    assert_eq!(searched["Resources"].as_array().unwrap().len(), 2);
}

#[actix_web::test]
async fn pagination_passes_through_in_both_directions() {
    let store = ThingStore::default();
    let seen = store.seen.clone();
    let app = test_app!(AppBuilder::new().register::<Thing, _>(store));
    let _ = seed!(app, "first");
    let _ = seed!(app, "second");
    let _ = seed!(app, "third");

    let listed: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(
            Method::GET,
            "/v2/Things?startIndex=2&count=5&excludedAttributes=meta",
        )
        .to_request(),
    )
    .await;

    let query = seen.lock().unwrap();
    let query = query.as_ref().unwrap();
    assert_eq!(query.start_index, Some(2));
    assert_eq!(query.count, Some(5));
    assert_eq!(query.excluded_attributes.as_deref(), Some("meta"));

    assert_eq!(listed["totalResults"], 3, "how many matched overall");
    assert_eq!(listed["startIndex"], 2);
    assert_eq!(listed["itemsPerPage"], 2, "how many this page carries");
}

fn bulk_app_builder() -> AppBuilder {
    AppBuilder::new()
        .register::<Thing, _>(ThingStore::default())
        .with_bulk(true, Some(10), None)
}

fn bulk_request(operations: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:BulkRequest"],
        "Operations": operations,
    })
}

#[actix_web::test]
async fn bulk_is_501_until_enabled() {
    let app = test_app!(AppBuilder::new().register::<Thing, _>(ThingStore::default()));

    let resp = actix_test::call_service(
        &app,
        request(Method::POST, "/v2/Bulk")
            .set_json(bulk_request(serde_json::json!([])))
            .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
}

#[actix_web::test]
async fn bulk_applies_a_mixed_batch() {
    let app = test_app!(bulk_app_builder());
    let id = seed!(app, "to-be-changed");
    let doomed = seed!(app, "to-be-deleted");

    let body: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Bulk")
            .set_json(bulk_request(serde_json::json!([
                {"method": "POST", "path": "/Things", "bulkId": "fresh",
                 "data": {"schemas": [Thing::SCHEMA], "displayName": "created"}},
                {"method": "PUT", "path": format!("/Things/{id}"),
                 "data": {"schemas": [Thing::SCHEMA], "displayName": "replaced"}},
                {"method": "PATCH", "path": format!("/Things/{id}"),
                 "data": {"schemas": [], "Operations": [
                     {"op": "replace", "path": "displayName", "value": "patched"}]}},
                {"method": "DELETE", "path": format!("/Things/{doomed}")},
            ])))
            .to_request(),
    )
    .await;

    assert_eq!(
        body["schemas"][0],
        "urn:ietf:params:scim:api:messages:2.0:BulkResponse"
    );
    let ops = body["Operations"].as_array().unwrap();
    assert_eq!(ops.len(), 4);
    assert_eq!(ops[0]["status"], "201");
    assert_eq!(ops[0]["bulkId"], "fresh");
    assert!(
        ops[0]["location"]
            .as_str()
            .unwrap()
            .starts_with("/v2/Things/"),
        "a created resource reports where it landed"
    );
    assert_eq!(ops[1]["status"], "200");
    assert_eq!(ops[2]["status"], "200");
    assert_eq!(ops[3]["status"], "204");
    assert!(ops.iter().all(|o| o.get("response").is_none()));

    let listed: serde_json::Value =
        actix_test::call_and_read_body_json(&app, request(Method::GET, "/v2/Things").to_request())
            .await;
    let names: Vec<&str> = listed["Resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["displayName"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"created"));
    assert!(names.contains(&"patched"));
    assert!(!names.contains(&"to-be-deleted"));
}

#[actix_web::test]
async fn bulk_resolves_bulk_id_references() {
    let app = test_app!(bulk_app_builder());

    let body: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Bulk")
            .set_json(bulk_request(serde_json::json!([
                {"method": "POST", "path": "/Things", "bulkId": "qwerty",
                 "data": {"schemas": [Thing::SCHEMA], "displayName": "first"}},
                {"method": "PUT", "path": "/Things/bulkId:qwerty",
                 "data": {"schemas": [Thing::SCHEMA], "displayName": "renamed"}},
            ])))
            .to_request(),
    )
    .await;

    let ops = body["Operations"].as_array().unwrap();
    assert_eq!(ops[0]["status"], "201");
    assert_eq!(
        ops[1]["status"], "200",
        "the PUT resolved bulkId:qwerty to the id the POST produced: {:?}",
        ops[1]
    );
    assert_eq!(ops[0]["location"], ops[1]["location"]);
}

#[actix_web::test]
async fn bulk_failures_are_reported_per_operation() {
    let app = test_app!(bulk_app_builder());

    let body: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Bulk")
            .set_json(bulk_request(serde_json::json!([
                {"method": "PUT", "path": "/Things/bulkId:never-defined",
                 "data": {"schemas": [Thing::SCHEMA], "displayName": "x"}},
                {"method": "POST", "path": "/Widgets",
                 "data": {"schemas": [], "displayName": "x"}},
                {"method": "GET", "path": "/Things"},
                {"method": "POST", "path": "/Things",
                 "data": {"schemas": [Thing::SCHEMA], "displayName": "survivor"}},
            ])))
            .to_request(),
    )
    .await;

    let ops = body["Operations"].as_array().unwrap();
    assert_eq!(ops.len(), 4, "one failure must not abort the batch");

    assert_eq!(ops[0]["status"], "400");
    assert_eq!(ops[0]["response"]["scimType"], "invalidValue");
    assert_eq!(ops[1]["status"], "404", "unregistered endpoint");
    assert_eq!(ops[2]["status"], "400", "GET is not a bulk method");
    assert_eq!(ops[3]["status"], "201", "the batch carried on");
}

#[actix_web::test]
async fn bulk_fail_on_errors_stops_early() {
    let app = test_app!(bulk_app_builder());

    let body: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::POST, "/v2/Bulk")
            .set_json(serde_json::json!({
                "schemas": ["urn:ietf:params:scim:api:messages:2.0:BulkRequest"],
                "failOnErrors": 1,
                "Operations": [
                    {"method": "POST", "path": "/Widgets", "data": {"schemas": []}},
                    {"method": "POST", "path": "/Things",
                     "data": {"schemas": [Thing::SCHEMA], "displayName": "never-runs"}},
                ],
            }))
            .to_request(),
    )
    .await;

    assert_eq!(body["Operations"].as_array().unwrap().len(), 1);

    let listed: serde_json::Value =
        actix_test::call_and_read_body_json(&app, request(Method::GET, "/v2/Things").to_request())
            .await;
    assert_eq!(listed["totalResults"], 0, "the second operation never ran");
}

#[actix_web::test]
async fn bulk_beyond_max_operations_is_413() {
    let app = test_app!(
        AppBuilder::new()
            .register::<Thing, _>(ThingStore::default())
            .with_bulk(true, Some(1), None)
    );

    let resp = actix_test::call_service(
        &app,
        request(Method::POST, "/v2/Bulk")
            .set_json(bulk_request(serde_json::json!([
                {"method": "DELETE", "path": "/Things/a"},
                {"method": "DELETE", "path": "/Things/b"},
            ])))
            .to_request(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let body: serde_json::Value = actix_test::read_body_json(resp).await;
    assert_eq!(body["scimType"], "tooLarge");
}

#[actix_web::test]
async fn bulk_is_advertised_once_enabled() {
    let app = test_app!(
        AppBuilder::new()
            .register::<Thing, _>(ThingStore::default())
            .with_bulk(true, Some(50), Some(1_048_576))
    );

    let config: serde_json::Value = actix_test::call_and_read_body_json(
        &app,
        request(Method::GET, "/v2/ServiceProviderConfig").to_request(),
    )
    .await;
    assert_eq!(config["bulk"]["supported"], true);
    assert_eq!(config["bulk"]["maxOperations"], 50);
    assert_eq!(config["bulk"]["maxPayloadSize"], 1_048_576);
}

mod metrics {
    use super::{AppBuilder, Method, StatusCode, Thing, ThingStore, actix_test, header, request};
    use crate::metrics::Metrics;

    fn series<'a>(
        gathered: &'a [prometheus::proto::MetricFamily],
        family: &str,
        labels: &[(&str, &str)],
    ) -> Option<&'a prometheus::proto::Metric> {
        gathered
            .iter()
            .find(|f| f.name() == family)?
            .get_metric()
            .iter()
            .find(|metric| {
                labels.iter().all(|(name, value)| {
                    metric
                        .get_label()
                        .iter()
                        .any(|label| label.name() == *name && label.value() == *value)
                })
            })
    }

    fn operations(
        metrics: &Metrics,
        resource_type: &str,
        operation: &str,
        source: &str,
        outcome: &str,
    ) -> u64 {
        let gathered = metrics.registry().gather();
        series(
            &gathered,
            "scimd_scim_operations_total",
            &[
                ("resource_type", resource_type),
                ("operation", operation),
                ("source", source),
                ("outcome", outcome),
            ],
        )
        .map_or(0, |metric| metric.get_counter().get_value() as u64)
    }

    fn http_endpoints(metrics: &Metrics) -> Vec<String> {
        metrics
            .registry()
            .gather()
            .iter()
            .filter(|f| f.name() == "scimd_http_requests_total")
            .flat_map(|f| f.get_metric())
            .filter_map(|metric| {
                metric
                    .get_label()
                    .iter()
                    .find(|label| label.name() == "endpoint")
                    .map(|label| label.value().to_owned())
            })
            .collect()
    }

    fn thing_app() -> AppBuilder {
        AppBuilder::new().register::<Thing, _>(ThingStore::default())
    }

    #[actix_web::test]
    async fn exposition_is_served_once_enabled() {
        let (app, _metrics) = metrics_app!(thing_app());
        actix_test::call_service(&app, request(Method::GET, "/v2/Things").to_request()).await;

        let resp = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/metrics").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = String::from_utf8(actix_test::read_body(resp).await.to_vec())
            .expect("the exposition is text");
        assert!(
            body.contains("scimd_registered_resource_types 1"),
            "the registry size is exposed: {body}"
        );
        assert!(
            body.contains("scimd_http_requests_total"),
            "the HTTP metrics are exposed too: {body}"
        );
    }

    #[actix_web::test]
    async fn exposition_is_reachable_with_a_trailing_slash() {
        let (app, _metrics) = metrics_app!(thing_app());

        let resp = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/metrics/").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn nothing_is_served_or_recorded_until_enabled() {
        let (app, metrics) = full_app!(thing_app(), false);

        let resp = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/metrics").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        actix_test::call_service(
            &app,
            request(Method::POST, "/v2/Things")
                .set_json(Thing::new("unwatched"))
                .to_request(),
        )
        .await;
        assert_eq!(
            operations(&metrics, "Thing", "create", "http", "success"),
            0,
            "a disabled server records nothing"
        );
    }

    #[actix_web::test]
    async fn a_scraper_needs_no_token_and_no_scim_accept() {
        let (app, _metrics) = metrics_app!(thing_app().bearer_token(Some("secret".into())));

        let resp = actix_test::call_service(
            &app,
            actix_test::TestRequest::get()
                .uri("/metrics")
                .insert_header((header::ACCEPT, "text/plain;version=0.0.4,*/*;q=0.1"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn operations_are_counted_per_resource_type_and_outcome() {
        let (app, metrics) = metrics_app!(thing_app());

        let created: serde_json::Value = actix_test::call_and_read_body_json(
            &app,
            request(Method::POST, "/v2/Things")
                .set_json(Thing::new("counted"))
                .to_request(),
        )
        .await;
        let id = created["id"].as_str().expect("created thing has an id");

        actix_test::call_service(
            &app,
            request(Method::GET, &format!("/v2/Things/{id}")).to_request(),
        )
        .await;
        actix_test::call_service(
            &app,
            request(
                Method::GET,
                "/v2/Things/00000000-0000-0000-0000-000000000000",
            )
            .to_request(),
        )
        .await;

        assert_eq!(
            operations(&metrics, "Thing", "create", "http", "success"),
            1
        );
        assert_eq!(operations(&metrics, "Thing", "get", "http", "success"), 1);
        assert_eq!(
            operations(&metrics, "Thing", "get", "http", "not_found"),
            1,
            "a miss is counted apart from a hit"
        );
    }

    #[actix_web::test]
    async fn unimplemented_operations_are_counted_as_such() {
        let (app, metrics) =
            metrics_app!(AppBuilder::new().register::<Thing, _>(super::SilentService {}));

        let resp =
            actix_test::call_service(&app, request(Method::GET, "/v2/Things").to_request()).await;
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);

        assert_eq!(
            operations(&metrics, "Thing", "list", "http", "not_implemented"),
            1
        );
        assert_eq!(operations(&metrics, "Thing", "list", "http", "success"), 0);
    }

    #[actix_web::test]
    async fn bulk_sub_operations_are_counted_apart_from_http_ones() {
        let (app, metrics) = metrics_app!(thing_app().with_bulk(true, Some(10), None));

        let resp = actix_test::call_service(
            &app,
            request(Method::POST, "/v2/Bulk")
                .set_json(super::bulk_request(serde_json::json!([
                    {"method": "POST", "path": "/Things",
                     "data": {"schemas": [<Thing as crate::Resource>::SCHEMA], "displayName": "one"}},
                    {"method": "POST", "path": "/Things",
                     "data": {"schemas": [<Thing as crate::Resource>::SCHEMA], "displayName": "two"}},
                    {"method": "DELETE", "path": "/Things/00000000-0000-0000-0000-000000000000"},
                ])))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        assert_eq!(
            operations(&metrics, "Thing", "create", "bulk", "success"),
            2
        );
        assert_eq!(
            operations(&metrics, "Thing", "delete", "bulk", "not_found"),
            1,
            "a failing operation inside a batch is visible"
        );
        assert_eq!(
            operations(&metrics, "Thing", "create", "http", "success"),
            0,
            "and does not blur into the ones the routes serve"
        );

        let gathered = metrics.registry().gather();
        let batches = series(&gathered, "scimd_bulk_operations_per_request", &[])
            .expect("the batch size was observed")
            .get_histogram();
        assert_eq!(batches.get_sample_count(), 1, "one batch");
        assert_eq!(batches.get_sample_sum(), 3.0, "carrying three operations");
    }

    #[actix_web::test]
    async fn http_series_are_keyed_by_route_pattern_not_path() {
        let (app, metrics) = metrics_app!(thing_app());

        for id in [
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ] {
            actix_test::call_service(
                &app,
                request(Method::GET, &format!("/v2/Things/{id}")).to_request(),
            )
            .await;
        }

        let endpoints = http_endpoints(&metrics);
        assert!(
            endpoints.contains(&"/v2/Things/{id}".to_owned()),
            "the pattern is what is recorded: {endpoints:?}"
        );
        assert!(
            !endpoints.iter().any(|e| e.contains("1111")),
            "no series carries a concrete id: {endpoints:?}"
        );
    }

    #[actix_web::test]
    async fn health_checks_are_not_counted() {
        let (app, metrics) = metrics_app!(thing_app());

        let resp = actix_test::call_service(
            &app,
            actix_test::TestRequest::get().uri("/health").to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        assert!(
            !http_endpoints(&metrics).contains(&"/health".to_owned()),
            "health checks stay out of the HTTP metrics"
        );
    }
}
