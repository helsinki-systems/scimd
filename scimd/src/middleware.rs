use actix_web::{body, dev, error::ResponseError as _, web};
use futures_util::{FutureExt as _, future::LocalBoxFuture};
use std::rc::Rc;
use std::{future, task};

use crate::State;

pub struct TokenService<S> {
    service: Rc<S>,
}

const AUTH_HEADER_NAME: &str = "Authorization";
const AUTH_TOKEN_PREFIX: &str = "Bearer ";

pub async fn validate_token(req: &dev::ServiceRequest) -> Result<(), super::Error> {
    let extracted_token = req
        .headers()
        .get(AUTH_HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(|t| {
            t.strip_prefix(AUTH_TOKEN_PREFIX)
                .ok_or(super::Error::Unauthorized)
                .map(|v| v.to_owned())
        })
        .map_or(Ok(None), |v| v.map(Some))?;

    let bt = req
        .app_data::<web::Data<State>>()
        .ok_or(super::Error::InternalError(anyhow::anyhow!(
            "No internal app state found"
        )))?
        .bearer_token
        .clone();

    if extracted_token == bt {
        Ok(())
    } else {
        Err(super::Error::Unauthorized)
    }
}

impl<S> dev::Service<dev::ServiceRequest> for TokenService<S>
where
    S: dev::Service<
            dev::ServiceRequest,
            Response = dev::ServiceResponse<body::BoxBody>,
            Error = actix_web::Error,
        > + 'static,
{
    type Response = dev::ServiceResponse<body::BoxBody>;
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, ctx: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        self.service.poll_ready(ctx)
    }

    fn call(&self, req: dev::ServiceRequest) -> Self::Future {
        let srv = self.service.clone();

        async move {
            match validate_token(&req).await {
                Ok(_) => Ok(srv.call(req).await?),
                Err(e) => Ok(req.into_response(e.error_response())),
            }
        }
        .boxed_local()
    }
}

pub struct TokenMiddleware {}
impl<S> dev::Transform<S, dev::ServiceRequest> for TokenMiddleware
where
    S: dev::Service<
            dev::ServiceRequest,
            Response = dev::ServiceResponse<body::BoxBody>,
            Error = actix_web::Error,
        > + 'static,
{
    type Response = dev::ServiceResponse<body::BoxBody>;
    type Error = actix_web::Error;
    type Future = future::Ready<Result<Self::Transform, Self::InitError>>;
    type Transform = TokenService<S>;
    type InitError = ();

    fn new_transform(&self, service: S) -> Self::Future {
        future::ready(Ok(TokenService {
            service: Rc::new(service),
        }))
    }
}

pub fn token() -> TokenMiddleware {
    TokenMiddleware {}
}
