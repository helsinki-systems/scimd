use std::collections::BTreeMap;
use std::sync::Mutex;

use actix_web::http::{Method, StatusCode, header};
use actix_web::test as actix_test;
use scimd::models::{ListResponse, Meta, Schema};
use scimd::{AppBuilder, Error, Resource, ResourceService, Result, ScimBuilder};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct Thing {
    schemas: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
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
    type Id = String;

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
        Schema::new(Self::SCHEMA, "Thing", "A thing", vec![])
    }
}

#[derive(Default)]
struct ThingStore {
    things: Mutex<BTreeMap<String, Thing>>,
    next: Mutex<u32>,
}

impl ThingStore {
    fn things(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Thing>> {
        self.things.lock().expect("store is not poisoned")
    }
}

#[scimd::async_trait]
impl ResourceService<Thing> for ThingStore {
    async fn list(&self, _filter: scimd::models::FilterQuery) -> Result<ListResponse<Thing>> {
        Ok(ListResponse::new(self.things().values().cloned().collect()))
    }

    async fn create(&self, mut thing: Thing) -> Result<Thing> {
        let id = thing.id.clone().unwrap_or_else(|| {
            let mut next = self.next.lock().expect("store is not poisoned");
            *next += 1;
            format!("thing-{next}")
        });
        thing.id = Some(id.clone());
        thing.meta = Some(Meta {
            created: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        });
        self.things().insert(id, thing.clone());
        Ok(thing)
    }

    async fn get(&self, id: String) -> Result<Thing> {
        self.things()
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::NotFound(id))
    }

    async fn delete(&self, id: String) -> Result<()> {
        self.things()
            .remove(&id)
            .map(|_| ())
            .ok_or_else(|| Error::NotFound(id))
    }
}

struct SilentService {}

#[scimd::async_trait]
impl ResourceService<Thing> for SilentService {}

fn request(method: Method, uri: &str) -> actix_test::TestRequest {
    actix_test::TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((header::ACCEPT, "application/scim+json"))
}

fn bulk_request(operations: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "schemas": ["urn:ietf:params:scim:api:messages:2.0:BulkRequest"],
        "Operations": operations,
    })
}

macro_rules! full_app {
    ($builder:expr, $metrics_enabled:expr) => {{
        let registry = scimd::MetricsRegistry::new();
        let app = $builder
            .metrics_registry(registry.clone())
            .with_metrics($metrics_enabled)
            .build();
        let (routes, enabled, prometheus) = crate::http_stack(&app).expect("the metrics register");
        let service = actix_test::init_service(scim_app!(routes, enabled, prometheus)).await;
        (service, registry)
    }};
}

mod metrics {
    use super::{
        AppBuilder, Method, ScimBuilder, StatusCode, Thing, ThingStore, actix_test, header, request,
    };
    use scimd::MetricsRegistry;

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
        registry: &MetricsRegistry,
        resource_type: &str,
        operation: &str,
        source: &str,
        outcome: &str,
    ) -> u64 {
        let gathered = registry.gather();
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

    fn http_endpoints(registry: &MetricsRegistry) -> Vec<String> {
        registry
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

    fn thing_core() -> ScimBuilder {
        ScimBuilder::new().register::<Thing, _>(ThingStore::default())
    }

    fn thing_app() -> AppBuilder {
        AppBuilder::new(thing_core())
    }

    #[actix_web::test]
    async fn exposition_is_served_once_enabled() {
        let (app, _metrics) = full_app!(thing_app(), true);
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
        let (app, _metrics) = full_app!(thing_app(), true);

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
        let families: Vec<String> = metrics
            .gather()
            .iter()
            .map(|f| f.name().to_owned())
            .collect();
        assert!(
            families.is_empty(),
            "a disabled server registers no collectors at all, got {families:?}"
        );
    }

    #[actix_web::test]
    async fn a_scraper_needs_no_token_and_no_scim_accept() {
        let (app, _metrics) = full_app!(thing_app().bearer_token(Some("secret".into())), true);

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
        let (app, metrics) = full_app!(thing_app(), true);

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
        let (app, metrics) = full_app!(
            AppBuilder::new(ScimBuilder::new().register::<Thing, _>(super::SilentService {})),
            true
        );

        actix_test::call_service(&app, request(Method::GET, "/v2/Things").to_request()).await;

        assert_eq!(
            operations(&metrics, "Thing", "list", "http", "not_implemented"),
            1
        );
        assert_eq!(operations(&metrics, "Thing", "list", "http", "success"), 0);
    }

    #[actix_web::test]
    async fn bulk_sub_operations_are_counted_apart_from_http_ones() {
        let (app, metrics) = full_app!(
            AppBuilder::new(thing_core().with_bulk(true, Some(10), None)),
            true
        );

        actix_test::call_service(
            &app,
            request(Method::POST, "/v2/Bulk")
                .set_json(super::bulk_request(serde_json::json!([
                    {"method": "POST", "path": "/Things",
                     "data": {"schemas": [<Thing as scimd::Resource>::SCHEMA], "displayName": "one"}},
                    {"method": "POST", "path": "/Things",
                     "data": {"schemas": [<Thing as scimd::Resource>::SCHEMA], "displayName": "two"}},
                    {"method": "DELETE", "path": "/Things/00000000-0000-0000-0000-000000000000"},
                ])))
                .to_request(),
        )
        .await;

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

        let gathered = metrics.gather();
        let batches = series(&gathered, "scimd_bulk_operations_per_request", &[])
            .expect("the batch size was observed")
            .get_histogram();
        assert_eq!(batches.get_sample_count(), 1, "one batch");
        assert_eq!(batches.get_sample_sum(), 3.0, "carrying three operations");
    }

    #[actix_web::test]
    async fn http_series_are_keyed_by_route_pattern_not_path() {
        let (app, metrics) = full_app!(thing_app(), true);

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
        let (app, metrics) = full_app!(thing_app(), true);

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

mod binds {
    use crate::{DEFAULT_BIND, Server};

    fn server() -> Server {
        Server::new(scimd::AppBuilder::default().build())
    }

    #[test]
    fn a_server_told_nothing_falls_back() {
        assert_eq!(server().tcp_binds(), vec![DEFAULT_BIND]);
    }

    #[test]
    fn a_unix_only_server_binds_no_tcp_socket() {
        let server = server().add_unix_bind("/run/scimd.sock".into());
        assert!(
            server.tcp_binds().is_empty(),
            "the fallback must not apply once any socket was asked for"
        );
    }

    #[test]
    fn explicit_binds_win() {
        let server = server()
            .add_bind("0.0.0.0:9000".into())
            .add_bind("[::1]:9001".into());
        assert_eq!(server.tcp_binds(), vec!["0.0.0.0:9000", "[::1]:9001"]);
    }
}
