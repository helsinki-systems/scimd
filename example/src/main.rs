//! Two resources served by the same server:
//!
//! - `User`, a resource type built into scimd, backed by a service that only logs.
//! - `Device`, a resource type defined entirely here, backed by an in-memory store.
//!
//! `Device` needs no changes to the framework: implementing [`scimd::Resource`] tells scimd how to
//! route and describe it, and implementing [`scimd::ResourceService`] tells it what to do.

use std::collections::BTreeMap;
use std::sync::Mutex;

use scimd::models::{Attributes, FilterQuery, ListResponse, Meta, PatchOp, Schema, ValOrEmpty};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<Meta>,
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
        Schema {
            id: Self::SCHEMA.into(),
            name: "Device".into(),
            description: "An example custom resource type".into(),
            attributes: vec![
                Attributes {
                    name: "displayName".into(),
                    type_: "string".into(),
                    description: Some("Human-readable name of the device.".into()),
                    required: Some(true),
                    mutability: Some("readWrite".into()),
                    returned: Some("default".into()),
                    ..Default::default()
                },
                Attributes {
                    name: "serialNumber".into(),
                    type_: "string".into(),
                    description: Some("Manufacturer-assigned serial number.".into()),
                    required: Some(true),
                    case_exact: Some(true),
                    mutability: Some("readWrite".into()),
                    returned: Some("default".into()),
                    uniqueness: Some("server".into()),
                    ..Default::default()
                },
            ],
            meta: Meta {
                resource_type: Some("Schema".into()),
                location: Some(format!("/v2/Schemas/{}", Self::SCHEMA)),
                ..Default::default()
            },
        }
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
    async fn list(&self, _filter: FilterQuery) -> scimd::Result<ListResponse<Device>> {
        Ok(ListResponse::new(
            self.devices().values().cloned().collect(),
        ))
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
    scimd::init_logging()?;

    let args = scimd::parse_args();
    let server = scimd::AppBuilder::new()
        .register::<scimd::models::User, _>(UserService {})
        .register::<Device, _>(DeviceService::default())
        .args(args)
        .bearer_token(dotenvy::var("EXAMPLE_SERVICE_BEARER_TOKEN").ok())
        .build();
    server.run().await?;

    Ok(())
}
