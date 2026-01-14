struct Service {}

#[scimd::async_trait]
impl scimd::Service for Service {
    async fn create_user(&self, user: scimd::models::User) -> scimd::Result<scimd::models::User> {
        tracing::info!("create_user: {:?}", user);
        Err(scimd::Error::MethodNotImplemented)
    }

    async fn replace_user(
        &self,
        id: uuid::Uuid,
        user: scimd::models::User,
    ) -> scimd::Result<scimd::models::User> {
        tracing::info!("replace_user: {} {:?}", id, user);
        Err(scimd::Error::NotFound(id))
    }

    async fn patch_user(
        &self,
        id: uuid::Uuid,
        op: scimd::models::PatchOp,
    ) -> scimd::Result<scimd::models::User> {
        tracing::info!("patch_user: {} {:?}", id, op);
        Err(scimd::Error::NotFound(id))
    }

    async fn delete_user(&self, id: uuid::Uuid) -> scimd::Result<()> {
        tracing::info!("delete_user: {}", id);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let args = scimd::parse_args();
    let server = scimd::AppBuilder::new(std::sync::Arc::new(Service {}))
        .args(args)
        .bearer_token(dotenvy::var("EXAMPLE_SERVICE_BEARER_TOKEN").ok())
        .build();
    server.run().await?;

    Ok(())
}
