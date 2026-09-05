//! [`scimd_types::Error`] as an actix-web error.
//!
//! [`Error`] is what a [`crate::ResourceService`] returns, so it has to hold for a backend that is
//! not actix-web; the orphan rule then makes the newtype the only way to give it `ResponseError`.

// The one place in the workspace that sees both `StatusCode` types - actix-web 4 is on http 0.2
// while the SCIM layer is on http 1 - so both are named explicitly.
use actix_web::http::StatusCode as ActixStatus;
use actix_web::{HttpResponse, ResponseError, http::header};

use scimd_core::headers::SCIM_MEDIA_TYPE;
pub use scimd_types::{Error, Result};

/// [`Error`] on its way out over HTTP. `?` converts into it.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct ScimError(#[from] pub Error);

impl ResponseError for ScimError {
    fn status_code(&self) -> ActixStatus {
        // The status crosses between the two `http` versions as a number. It cannot fail - a
        // `StatusCode` only ever holds 100..=999, which is exactly what `from_u16` accepts - and
        // `every_status_survives_the_crossing` below pins that.
        ActixStatus::from_u16(self.0.status().as_u16())
            .unwrap_or(ActixStatus::INTERNAL_SERVER_ERROR)
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .append_header((header::CONTENT_TYPE, SCIM_MEDIA_TYPE))
            .json(self.0.to_scim_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_survives_the_crossing() {
        let all = [
            Error::InternalError(anyhow::anyhow!("boom")),
            Error::MethodNotImplemented,
            Error::Conflict,
            Error::Unauthorized,
            Error::NotFound(String::new()),
            Error::BadRequest(String::new()),
            Error::InvalidValue(String::new()),
            Error::InvalidFilter(String::new()),
            Error::PayloadTooLarge(String::new()),
            Error::NotAcceptable(String::new()),
        ];
        for error in all {
            let expected = error.status().as_u16();
            let display = error.to_string();
            assert_eq!(
                ScimError::from(error).status_code().as_u16(),
                expected,
                "'{display}' answers {expected}, which did not survive into actix's StatusCode"
            );
        }
    }
}
