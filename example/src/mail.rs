//! Mail accounts: users that own a mailbox, served on `/Users`.
//!
//! The mailbox is not a resource type of its own. It is part of the user, described by a schema
//! extension, and it follows the user's lifecycle: `POST` creates it, `active: false` suspends it,
//! `DELETE` removes it. A real service would call its mail server where this one logs.

use std::collections::BTreeMap;
use std::sync::Mutex;

use scimd::models::{
    Attributes, Extension, FilterQuery, ListResponse, Meta, PatchOp, Schema, User, ValOrEmpty,
};
use scimd::{Resource, ResourceService};

/// The built-in `User` plus the mailbox extension.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct MailUser {
    #[serde(flatten)]
    pub user: User,
    #[serde(rename = "urn:example:params:scim:schemas:extension:mailbox:User")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailbox: Option<Mailbox>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Mailbox {
    /// In MiB. `None` means the mail server's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<u64>,
}

impl Mailbox {
    const SCHEMA: &'static str = "urn:example:params:scim:schemas:extension:mailbox:User";

    fn schema() -> Schema {
        Schema::new(
            Self::SCHEMA,
            "Mailbox",
            "The mailbox that belongs to a user",
            vec![Attributes::new("quota", "integer").description("Mailbox quota in MiB.")],
        )
    }
}

impl Resource for MailUser {
    type Id = String;
    const RESOURCE_TYPE: &'static str = User::RESOURCE_TYPE;
    const ENDPOINT: &'static str = User::ENDPOINT;
    const SCHEMA: &'static str = User::SCHEMA;

    fn id(&self) -> Option<&Self::Id> {
        self.user.id()
    }

    fn meta(&self) -> Option<&Meta> {
        self.user.meta()
    }

    fn set_meta(&mut self, meta: Meta) {
        self.user.set_meta(meta);
    }

    fn schema() -> Schema {
        User::schema()
    }

    fn extensions() -> Vec<Extension> {
        let mut extensions = User::extensions();
        extensions.push(Extension::optional(Mailbox::schema()));
        extensions
    }
}

#[derive(Default)]
pub struct MailUsers {
    users: Mutex<BTreeMap<String, MailUser>>,
}

impl MailUsers {
    fn users(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, MailUser>> {
        self.users.lock().expect("user store is not poisoned")
    }
}

fn log_activation(user: &MailUser) {
    match user.user.active {
        Some(false) => tracing::info!("suspended mailbox of {}", user.user.user_name),
        _ => tracing::info!("resumed mailbox of {}", user.user.user_name),
    }
}

#[scimd::async_trait]
impl ResourceService<MailUser> for MailUsers {
    async fn list(&self, query: FilterQuery) -> scimd::Result<ListResponse<MailUser>> {
        let users = self.users().values().cloned().collect();
        scimd::filter::list_response(users, &query)
    }

    async fn create(&self, mut user: MailUser) -> scimd::Result<MailUser> {
        let mut users = self.users();
        if users
            .values()
            .any(|u| u.user.user_name.eq_ignore_ascii_case(&user.user.user_name))
        {
            return Err(scimd::Error::Conflict);
        }
        let id = uuid::Uuid::new_v4().to_string();
        user.user.id = Some(id.clone());
        user.user.active.get_or_insert(true);
        user.mailbox.get_or_insert_with(Mailbox::default);
        tracing::info!("created mailbox for {}", user.user.user_name);
        users.insert(id, user.clone());
        Ok(user)
    }

    async fn get(&self, id: String) -> scimd::Result<MailUser> {
        self.users()
            .get(&id)
            .cloned()
            .ok_or(scimd::Error::NotFound(id))
    }

    async fn replace(&self, id: String, mut user: MailUser) -> scimd::Result<MailUser> {
        let mut users = self.users();
        let current = users
            .get_mut(&id)
            .ok_or_else(|| scimd::Error::NotFound(id.clone()))?;
        user.user.id = Some(id);
        user.user.active.get_or_insert(true);
        if user.user.active != current.user.active {
            log_activation(&user);
        }
        *current = user.clone();
        Ok(user)
    }

    async fn patch(&self, id: String, op: PatchOp) -> scimd::Result<ValOrEmpty<MailUser>> {
        let mut users = self.users();
        let user = users
            .get_mut(&id)
            .ok_or_else(|| scimd::Error::NotFound(id.clone()))?;
        for operation in &op.operations {
            match operation.attr().as_str() {
                "active" => {
                    let active = operation.bool_value().ok_or_else(|| {
                        scimd::Error::InvalidValue("active must be a boolean".into())
                    })?;
                    if user.user.active != Some(active) {
                        user.user.active = Some(active);
                        log_activation(user);
                    }
                }
                "quota" => {
                    let quota = operation.value.as_ref().and_then(|v| v.as_u64());
                    user.mailbox.get_or_insert_with(Mailbox::default).quota = quota;
                }
                other => tracing::debug!("ignoring patch on '{other}' for user {id}"),
            }
        }
        Ok(ValOrEmpty::Val(user.clone()))
    }

    async fn delete(&self, id: String) -> scimd::Result<()> {
        let user = self.users().remove(&id).ok_or(scimd::Error::NotFound(id))?;
        tracing::info!("deleted mailbox of {}", user.user.user_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scimd::models::patchop::Operation;

    fn jdoe() -> MailUser {
        serde_json::from_value(serde_json::json!({
            "schemas": [User::SCHEMA, Mailbox::SCHEMA],
            "userName": "jdoe",
            Mailbox::SCHEMA: { "quota": 1024 },
        }))
        .unwrap()
    }

    fn patch(operations: Vec<Operation>) -> PatchOp {
        PatchOp {
            schemas: vec!["urn:ietf:params:scim:api:messages:2.0:PatchOp".into()],
            operations,
        }
    }

    #[test]
    fn the_mailbox_extension_round_trips_next_to_the_core_attributes() {
        let json = serde_json::to_value(jdoe()).unwrap();
        assert_eq!(json["userName"], "jdoe");
        assert_eq!(json[Mailbox::SCHEMA]["quota"], 1024);
    }

    #[tokio::test]
    async fn the_mailbox_follows_the_user_lifecycle() {
        let service = MailUsers::default();
        let id = service.create(jdoe()).await.unwrap().user.id.unwrap();
        assert!(matches!(
            service.create(jdoe()).await,
            Err(scimd::Error::Conflict)
        ));

        let suspend = patch(vec![Operation::replace(
            "active".into(),
            Some(serde_json::json!(false)),
        )]);
        service.patch(id.clone(), suspend).await.unwrap();
        assert_eq!(
            service.get(id.clone()).await.unwrap().user.active,
            Some(false)
        );

        let quota = patch(vec![Operation::replace(
            format!("{}:quota", Mailbox::SCHEMA),
            Some(serde_json::json!(2048)),
        )]);
        service.patch(id.clone(), quota).await.unwrap();
        let mailbox = service.get(id.clone()).await.unwrap().mailbox.unwrap();
        assert_eq!(mailbox.quota, Some(2048));

        service.delete(id.clone()).await.unwrap();
        assert!(matches!(
            service.get(id).await,
            Err(scimd::Error::NotFound(_))
        ));
    }
}
