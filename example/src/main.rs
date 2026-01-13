struct Service {}

#[scimd::async_trait]
impl scimd::Service for Service {
    async fn create_user(&self, user: scimd::models::User) -> scimd::Result<scimd::HttpResponse> {
        tracing::info!("create_user: {:?}", user);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn replace_user(
        &self,
        id: uuid::Uuid,
        user: scimd::models::User,
    ) -> scimd::Result<scimd::HttpResponse> {
        tracing::info!("replace_user: {} {:?}", id, user);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn patch_user(
        &self,
        id: uuid::Uuid,
        op: scimd::models::PatchOp,
    ) -> scimd::Result<scimd::HttpResponse> {
        tracing::info!("patch_user: {} {:?}", id, op);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn delete_user(&self, id: uuid::Uuid) -> scimd::Result<scimd::HttpResponse> {
        tracing::info!("delete_user: {}", id);
        Ok(scimd::HttpResponse::Ok().finish())
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
