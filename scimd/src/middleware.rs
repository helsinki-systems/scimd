//! The checks every request to the SCIM scope passes before it reaches a handler.
//!
//! Content negotiation lives here rather than in an actix guard on the scope: a guard that does
//! not match makes the route disappear, so a client with the wrong `Accept` would be told the
//! endpoint does not exist instead of that its `Accept` is the problem.

use actix_web::{body, dev, error::ResponseError as _, http::header, web};
use futures_util::{FutureExt as _, future::LocalBoxFuture};
use std::rc::Rc;
use std::{future, task};

use crate::State;

pub struct ScimService<S> {
    service: Rc<S>,
}

const AUTH_HEADER_NAME: &str = "Authorization";
const AUTH_TOKEN_PREFIX: &str = "Bearer ";

/// What scimd sends. RFC 7644 section 3.1.
pub const SCIM_MEDIA_TYPE: &str = "application/scim+json";
/// Which that same section allows a SCIM server to answer with as well.
pub const JSON_MEDIA_TYPE: &str = "application/json";

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

/// Whether the client will take what we are going to send.
///
/// Per RFC 7231 section 5.3.2 an absent or empty `Accept` means the client accepts anything, and
/// `*/*` and `application/*` both cover [`SCIM_MEDIA_TYPE`]. Media types are compared
/// case-insensitively, as that section requires.
pub fn validate_accept(req: &dev::ServiceRequest) -> Result<(), super::Error> {
    let Some(accept) = req.headers().get(header::ACCEPT) else {
        return Ok(());
    };
    let accept = accept
        .to_str()
        .map_err(|_| super::Error::NotAcceptable("the Accept header is not valid text".into()))?;
    if accept.trim().is_empty() {
        return Ok(());
    }

    let acceptable = accept.split(',').any(|range| {
        let range = range.split(';').next().unwrap_or_default().trim();
        ["*/*", "application/*", SCIM_MEDIA_TYPE, JSON_MEDIA_TYPE]
            .iter()
            .any(|known| range.eq_ignore_ascii_case(known))
    });

    if acceptable {
        Ok(())
    } else {
        Err(super::Error::NotAcceptable(format!(
            "this endpoint answers {SCIM_MEDIA_TYPE}, which '{accept}' does not accept"
        )))
    }
}

impl<S> dev::Service<dev::ServiceRequest> for ScimService<S>
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
            // Authorization first: an unauthenticated client learns nothing from the reply beyond
            // that it is unauthenticated.
            match validate_token(&req)
                .await
                .and_then(|()| validate_accept(&req))
            {
                Ok(()) => Ok(srv.call(req).await?),
                Err(e) => Ok(req.into_response(e.error_response())),
            }
        }
        .boxed_local()
    }
}

pub struct ScimMiddleware {}
impl<S> dev::Transform<S, dev::ServiceRequest> for ScimMiddleware
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
    type Transform = ScimService<S>;
    type InitError = ();

    fn new_transform(&self, service: S) -> Self::Future {
        future::ready(Ok(ScimService {
            service: Rc::new(service),
        }))
    }
}

pub fn scim() -> ScimMiddleware {
    ScimMiddleware {}
}
