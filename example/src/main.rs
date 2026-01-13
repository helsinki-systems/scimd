struct Service {}

#[scimd::async_trait]
impl scimd::Service for Service {
    async fn create_user(
        &self,
        user: scimd::models::User,
    ) -> Result<scimd::HttpResponse, Box<dyn std::error::Error>> {
        tracing::info!("create_user: {:?}", user);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn replace_user(
        &self,
        id: uuid::Uuid,
        user: scimd::models::User,
    ) -> Result<scimd::HttpResponse, Box<dyn std::error::Error>> {
        tracing::info!("replace_user: {} {:?}", id, user);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn patch_user(
        &self,
        id: uuid::Uuid,
        op: scimd::models::PatchOp,
    ) -> Result<scimd::HttpResponse, Box<dyn std::error::Error>> {
        tracing::info!("patch_user: {} {:?}", id, op);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn delete_user(
        &self,
        id: uuid::Uuid,
    ) -> Result<scimd::HttpResponse, Box<dyn std::error::Error>> {
        tracing::info!("delete_user: {}", id);
        Ok(scimd::HttpResponse::Ok().finish())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let args = scimd::parse_args();
    let server = scimd::AppBuilder::new(std::sync::Arc::new(Service {}))
        .args(args)
        .bearer_token(dotenvy::var("EXAMPLE_SERVICE_BEARER_TOKEN").ok())
        .build();
    server.run().await?;

    Ok(())
}
