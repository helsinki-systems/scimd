use actix_web::{HttpResponse, ResponseError, http::StatusCode, http::header};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an unspecified internal error occurred: {0}")]
    InternalError(#[from] anyhow::Error),

    #[error("Method not implemented")]
    MethodNotImplemented,

    #[error("Conflict")]
    Conflict,

    #[error("Unauthorized")]
    Unauthorized,

    // RFC 7643 defines `id` as opaque and each resource type chooses its own `Resource::Id`
    #[error("Resource {0} not found")]
    NotFound(String),

    #[error("Bad request. Details={0}")]
    BadRequest(String),

    #[error("Invalid value. Details={0}")]
    InvalidValue(String),

    #[error("Payload too large. Details={0}")]
    PayloadTooLarge(String),
}

impl Error {
    pub(crate) fn get_detail(&self) -> Option<String> {
        match &self {
            Self::InternalError(_) | Self::Unauthorized => None,
            Self::BadRequest(v) | Self::InvalidValue(v) | Self::PayloadTooLarge(v) => {
                Some(v.to_owned())
            }
            _ => Some(self.to_string()),
        }
    }

    pub(crate) fn scim_type(&self) -> Option<&'static str> {
        match &self {
            Self::InvalidValue(_) => Some("invalidValue"),
            Self::PayloadTooLarge(_) => Some("tooLarge"),
            _ => None,
        }
    }

    pub(crate) fn to_scim_error(&self) -> crate::models::Error {
        crate::models::Error::new(self.status_code())
            .with_scim_type(self.scim_type())
            .with_detail(self.get_detail())
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match &self {
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MethodNotImplemented => StatusCode::NOT_IMPLEMENTED,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) | Self::InvalidValue(_) => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .append_header((header::CONTENT_TYPE, "application/scim+json"))
            .json(self.to_scim_error())
    }
}

// Short hand alias, which allows you to use just Result<T>
pub type Result<T> = std::result::Result<T, Error>;
