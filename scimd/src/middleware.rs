//! Authentication and content negotiation for every request to the SCIM scope, wrapping the checks
//! in [`scimd_core::headers`].

use actix_web::{body, dev, error::ResponseError as _, http::header};
use futures_util::{FutureExt as _, future::LocalBoxFuture};
use std::rc::Rc;
use std::{future, task};

use scimd_core::headers;

pub use scimd_core::headers::{JSON_MEDIA_TYPE, SCIM_MEDIA_TYPE};

/// What a mount checks the `Authorization` header against.
#[derive(Clone, Debug)]
pub enum Auth {
    /// Every request must present exactly this bearer token.
    Bearer(Rc<str>),
    /// No token is configured, and a request presenting one anyway is refused - see
    /// [`scimd_core::headers::validate_token`]. This is *not* "no authentication".
    None,
    /// The host authenticated the request before it reached the mount, so the header is not read
    /// at all - which is what a host that authenticates upstream wants.
    Upstream,
}

impl Auth {
    /// So that a caller never has to name `Rc`.
    #[must_use]
    pub fn bearer(token: &str) -> Self {
        Self::Bearer(Rc::from(token))
    }
}

pub struct ScimService<S> {
    service: Rc<S>,
    auth: Auth,
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
        let auth = self.auth.clone();

        async move {
            let header = |name| req.headers().get(name).map(header::HeaderValue::as_bytes);
            let authenticated = match &auth {
                // Not even to refuse it: the host already read the header, and what it means is
                // between the host and its own identity provider.
                Auth::Upstream => Ok(()),
                Auth::Bearer(token) => {
                    headers::validate_token(header(header::AUTHORIZATION.as_str()), Some(token))
                }
                Auth::None => headers::validate_token(header(header::AUTHORIZATION.as_str()), None),
            };
            match authenticated
                .and_then(|()| headers::validate_accept(header(header::ACCEPT.as_str())))
            {
                Ok(()) => Ok(srv.call(req).await?),
                Err(e) => Ok(req.into_response(crate::ScimError::from(e).error_response())),
            }
        }
        .boxed_local()
    }
}

pub struct ScimMiddleware {
    auth: Auth,
}

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
            auth: self.auth.clone(),
        }))
    }
}

/// What to check is injected rather than looked up out of app data, so that the middleware is
/// usable wherever a host puts it.
pub fn scim(auth: Auth) -> ScimMiddleware {
    ScimMiddleware { auth }
}
