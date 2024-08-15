struct Service {}

#[scimd::async_trait]
impl scimd::Service for Service {
    async fn create_user(
        &self,
        user: scimd::models::User,
    ) -> Result<scimd::HttpResponse, Box<dyn std::error::Error>> {
        scimd::info!("create_user: {:?}", user);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn replace_user(
        &self,
        id: scimd::Uuid,
        user: scimd::models::User,
    ) -> Result<scimd::HttpResponse, Box<dyn std::error::Error>> {
        scimd::info!("replace_user: {} {:?}", id, user);
        Ok(scimd::HttpResponse::Ok().finish())
    }

    async fn delete_user(
        &self,
        id: scimd::Uuid,
    ) -> Result<scimd::HttpResponse, Box<dyn std::error::Error>> {
        scimd::info!("delete_user: {}", id);
        Ok(scimd::HttpResponse::Ok().finish())
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();

    let service = std::sync::Arc::new(Service {});
    let server = scimd::AppBuilder::new(service)
        .worker(1)
        .bind("127.0.0.1:5000".to_string())
        .bearer_token(std::env::var("EXAMPLE_SERVICE_BEARER_TOKEN").ok())
        .build();
    server.run().await?;

    Ok(())
}
