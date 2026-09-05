use scimd_types::Resource;
use scimd_types::models::{BulkRequest, Meta, Schema};

use crate::resource::finalize;
use crate::{Error, ScimBuilder, normalise_path};

#[derive(serde::Serialize, serde::Deserialize, Default, Debug)]
struct Thing {
    id: Option<String>,
    meta: Option<Meta>,
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

#[test]
fn finalize_fills_only_the_meta_the_service_left_out() {
    let mut thing = Thing {
        id: Some("abc".into()),
        meta: None,
    };
    finalize(&mut thing, "/v2");
    let meta = thing.meta.expect("meta was filled in");
    assert_eq!(meta.resource_type.as_deref(), Some("Thing"));
    assert_eq!(meta.location.as_deref(), Some("/v2/Things/abc"));

    let mut thing = Thing {
        id: Some("abc".into()),
        meta: Some(Meta {
            created: Some("2026-01-01T00:00:00Z".into()),
            location: Some("https://example.test/custom".into()),
            ..Default::default()
        }),
    };
    finalize(&mut thing, "/v2");
    let meta = thing.meta.expect("meta survived");
    assert_eq!(
        meta.location.as_deref(),
        Some("https://example.test/custom"),
        "a location set by the service must not be overwritten"
    );
    assert_eq!(meta.created.as_deref(), Some("2026-01-01T00:00:00Z"));
    assert_eq!(meta.resource_type.as_deref(), Some("Thing"));

    let mut thing = Thing::default();
    finalize(&mut thing, "/v2");
    assert_eq!(thing.meta.expect("meta was filled in").location, None);
}

#[test]
fn a_configured_path_is_normalised_to_one_leading_slash() {
    for (given, expected) in [
        ("/v2", "/v2"),
        ("v2", "/v2"),
        ("/scim/v2/", "/scim/v2"),
        ("//scim/v2", "/scim/v2"),
        ("///scim/v2//", "/scim/v2"),
        ("/scim//v2", "/scim/v2"),
        ("scim//v2//Things", "/scim/v2/Things"),
    ] {
        assert_eq!(
            normalise_path(given).as_deref(),
            Some(expected),
            "normalising {given}"
        );
    }
    for empty in ["", "  ", "/", "///"] {
        assert_eq!(normalise_path(empty), None, "normalising {empty:?}");
    }
}

#[tokio::test]
async fn bulk_is_refused_until_it_is_enabled() {
    let scim = ScimBuilder::new().build();
    assert!(matches!(
        scim.bulk(BulkRequest::default()).await,
        Err(Error::MethodNotImplemented)
    ));

    let scim = ScimBuilder::new().with_bulk(true, None, None).build();
    assert!(scim.bulk(BulkRequest::default()).await.is_ok());
}

#[test]
fn finalize_writes_the_location_the_server_was_configured_for() {
    for base in ["/v2", "/scim/v2", "/tenants/acme/scim/v2"] {
        let mut thing = Thing {
            id: Some("abc".into()),
            meta: None,
        };
        finalize(&mut thing, base);
        assert_eq!(
            thing.meta.expect("meta was filled in").location,
            Some(format!("{base}/Things/abc"))
        );
    }
}
