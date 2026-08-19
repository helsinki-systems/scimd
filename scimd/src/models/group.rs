use super::meta::Meta;
use super::schema::Schema;
use crate::Resource;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Group {
    pub schemas: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "externalId", skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<Member>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

impl Resource for Group {
    type Id = String;

    const RESOURCE_TYPE: &'static str = "Group";
    const ENDPOINT: &'static str = "/Groups";
    const SCHEMA: &'static str = "urn:ietf:params:scim:schemas:core:2.0:Group";

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
        super::schemas::group()
    }
}

impl Default for Group {
    fn default() -> Self {
        Group {
            schemas: vec![Self::SCHEMA.to_string()],
            id: None,
            external_id: None,
            display_name: String::new(),
            members: None,
            meta: None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Default)]
pub struct Member {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(rename = "$ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}
