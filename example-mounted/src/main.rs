//! scimd mounted into a host's own actix-web server.
//!
//! The other example (`cargo run -p example`) lets scimd own the process. This one is the case
//! `App::configure` exists for: a backend that already has an `HttpServer` and wants SCIM
//! alongside its own routes rather than in a second process.
//!
//! ```console
//! $ cargo run -p example-mounted
//! $ curl localhost:8081/api/widgets  # the host's own route, no SCIM headers
//! $ curl -H 'Authorization: Bearer s3cret' localhost:8081/scim/v2/Users
//! $ curl -i -H 'Authorization: Bearer s3cret' localhost:8081/scim/v2/Nope  # a SCIM 404
//! $ curl -i localhost:8081/scim/v2/Nope  # 401 - auth runs before the scope's 404
//! ```

use std::collections::BTreeMap;
use std::sync::Mutex;

use actix_web::{App, HttpResponse, HttpServer, web};
use scimd::models::{FilterQuery, ListResponse, Meta, User};
use scimd::{Error, ResourceService, Result};

const BEARER_TOKEN: &str = "s3cret";

/// Everything scimd publishes - `meta.location`, `/ResourceTypes`, `/Schemas` - is written
/// relative to this, so a client following a link it was given lands back here.
const SCIM_BASE: &str = "/scim/v2";

/// A real backend has a database here.
#[derive(Default)]
struct Users {
    users: Mutex<BTreeMap<String, User>>,
}

impl Users {
    fn store(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, User>> {
        self.users.lock().expect("the store is not poisoned")
    }
}

#[scimd::async_trait]
impl ResourceService<User> for Users {
    async fn list(&self, _filter: FilterQuery) -> Result<ListResponse<User>> {
        Ok(ListResponse::new(self.store().values().cloned().collect()))
    }

    async fn create(&self, mut user: User) -> Result<User> {
        let id = user
            .id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        user.id = Some(id.clone());
        user.meta = Some(Meta {
            created: Some("2026-01-01T00:00:00Z".into()),
            ..Default::default()
        });
        self.store().insert(id, user.clone());
        Ok(user)
    }

    async fn get(&self, id: String) -> Result<User> {
        self.store()
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::NotFound(id))
    }

    async fn delete(&self, id: String) -> Result<()> {
        self.store()
            .remove(&id)
            .map(|_| ())
            .ok_or_else(|| Error::NotFound(id))
    }
}

/// Whatever the host was already serving.
async fn widgets() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({ "widgets": ["left-handed", "right-handed"] }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // A host brings its own subscriber; `scimd_server::init_logging` is for the daemon.
    tracing_subscriber::fmt::init();

    // Built once, outside the per-worker factory: `configure()` hands back a cheap-to-clone value,
    // and building it per worker would re-register the metrics.
    let core = scimd::ScimBuilder::new()
        .register::<User, _>(Users::default())
        .base_path(SCIM_BASE);
    let scim = scimd::AppBuilder::new(core)
        .bearer_token(Some(BEARER_TOKEN.to_owned()))
        .build();
    let scim_routes = scim.configure();

    tracing::info!("host API on /api, SCIM on {SCIM_BASE}, listening on 127.0.0.1:8081");

    HttpServer::new(move || {
        App::new()
            // The host's own routes: no SCIM media type, no bearer token - scimd's middleware is
            // scoped to its own mount.
            .service(web::scope("/api").route("/widgets", web::get().to(widgets)))
            .route("/health", web::get().to(|| async { "OK" }))
            .configure(scim_routes.clone())
    })
    .bind("127.0.0.1:8081")?
    .run()
    .await?;

    Ok(())
}
