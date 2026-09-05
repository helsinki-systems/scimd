//! Two resources served by the same server:
//!
//! - `User`, a resource type built into scimd, backed by a service that only logs.
//! - `Device`, a resource type defined entirely here, backed by an in-memory store.
//!
//! `Device` needs no changes to the framework: implementing [`scimd::Resource`] tells scimd how to
//! route and describe it, and implementing [`scimd::ResourceService`] tells it what to do.

use std::collections::BTreeMap;
use std::sync::Mutex;

use scimd::models::{
    Attributes, Extension, FilterQuery, ListResponse, Meta, PatchOp, Schema, ValOrEmpty,
};
use scimd::{Resource, ResourceService};

struct UserService {}

#[scimd::async_trait]
impl ResourceService<scimd::models::User> for UserService {
    async fn create(&self, user: scimd::models::User) -> scimd::Result<scimd::models::User> {
        tracing::info!("create user: {:?}", user);
        Err(scimd::Error::MethodNotImplemented)
    }

    async fn replace(
        &self,
        id: String,
        user: scimd::models::User,
    ) -> scimd::Result<scimd::models::User> {
        tracing::info!("replace user: {} {:?}", id, user);
        Err(scimd::Error::NotFound(id))
    }

    async fn patch(
        &self,
        id: String,
        op: PatchOp,
    ) -> scimd::Result<ValOrEmpty<scimd::models::User>> {
        tracing::info!("patch user: {} {:?}", id, op);
        Err(scimd::Error::NotFound(id))
    }

    async fn delete(&self, id: String) -> scimd::Result<()> {
        tracing::info!("delete user: {}", id);
        Ok(())
    }
}

/// A resource type that is not part of SCIM's predefined set.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct Device {
    schemas: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<uuid::Uuid>,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "serialNumber")]
    serial_number: String,
    #[serde(rename = "urn:example:params:scim:schemas:extension:lease:Device")]
    #[serde(skip_serializing_if = "Option::is_none")]
    lease: Option<Lease>,
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<Meta>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
struct Lease {
    #[serde(rename = "leasedUntil", skip_serializing_if = "Option::is_none")]
    leased_until: Option<String>,
}

impl Lease {
    const SCHEMA: &'static str = "urn:example:params:scim:schemas:extension:lease:Device";

    fn schema() -> Schema {
        Schema::new(
            Self::SCHEMA,
            "DeviceLease",
            "Leasing information for a device",
            vec![
                Attributes::new("leasedUntil", "dateTime")
                    .description("When the lease on this device expires."),
            ],
        )
    }
}

impl Resource for Device {
    // Unlike the built-in User and Group, which key on opaque strings, Device keys on UUIDs
    type Id = uuid::Uuid;

    const RESOURCE_TYPE: &'static str = "Device";
    const ENDPOINT: &'static str = "/Devices";
    const SCHEMA: &'static str = "urn:example:params:scim:schemas:Device";

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
            "Device",
            "An example custom resource type",
            vec![
                Attributes::new("displayName", "string")
                    .description("Human-readable name of the device.")
                    .required(),
                Attributes::new("serialNumber", "string")
                    .description("Manufacturer-assigned serial number.")
                    .required()
                    .case_exact()
                    .uniqueness("server"),
            ],
        )
    }

    fn extensions() -> Vec<Extension> {
        vec![Extension::optional(Lease::schema())]
    }
}

#[derive(Default)]
struct DeviceService {
    devices: Mutex<BTreeMap<uuid::Uuid, Device>>,
}

impl DeviceService {
    fn devices(&self) -> std::sync::MutexGuard<'_, BTreeMap<uuid::Uuid, Device>> {
        self.devices.lock().expect("device store is not poisoned")
    }
}

#[scimd::async_trait]
impl ResourceService<Device> for DeviceService {
    async fn list(&self, query: FilterQuery) -> scimd::Result<ListResponse<Device>> {
        let filter = query
            .filter
            .as_deref()
            .map(scimd::filter::parse)
            .transpose()?;
        let mut matched = vec![];
        for device in self.devices().values() {
            let keep = match &filter {
                None => true,
                Some(filter) => filter.matches_resource(device)?,
            };
            if keep {
                matched.push(device.clone());
            }
        }
        Ok(ListResponse::new(matched))
    }

    async fn create(&self, mut device: Device) -> scimd::Result<Device> {
        let id = device.id.unwrap_or_else(uuid::Uuid::new_v4);
        device.id = Some(id);
        self.devices().insert(id, device.clone());
        tracing::info!("created device {}", id);
        Ok(device)
    }

    async fn get(&self, id: uuid::Uuid) -> scimd::Result<Device> {
        self.devices()
            .get(&id)
            .cloned()
            .ok_or_else(|| scimd::Error::NotFound(id.to_string()))
    }

    async fn replace(&self, id: uuid::Uuid, mut device: Device) -> scimd::Result<Device> {
        let mut devices = self.devices();
        if !devices.contains_key(&id) {
            return Err(scimd::Error::NotFound(id.to_string()));
        }
        device.id = Some(id);
        devices.insert(id, device.clone());
        Ok(device)
    }

    async fn delete(&self, id: uuid::Uuid) -> scimd::Result<()> {
        self.devices()
            .remove(&id)
            .map(|_| ())
            .ok_or_else(|| scimd::Error::NotFound(id.to_string()))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    scimd_server::init_logging()?;

    let args = scimd_server::parse_args();
    let scim = scimd::ScimBuilder::new()
        .register::<scimd::models::User, _>(UserService {})
        .register::<Device, _>(DeviceService::default())
        .with_bulk(true, Some(50), Some(1024 * 1024))
        .with_filter(true, None);
    let app = scimd::AppBuilder::new(scim)
        .with_metrics(true)
        .bearer_token(dotenvy::var("EXAMPLE_SERVICE_BEARER_TOKEN").ok())
        .build();
    scimd_server::Server::new(app).args(args).run().await?;

    Ok(())
}
