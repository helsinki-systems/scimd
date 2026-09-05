use http::StatusCode;

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

    #[error("Invalid filter. Details={0}")]
    InvalidFilter(String),

    #[error("Payload too large. Details={0}")]
    PayloadTooLarge(String),

    #[error("Not acceptable. Details={0}")]
    NotAcceptable(String),
}

impl Error {
    pub fn get_detail(&self) -> Option<String> {
        match &self {
            Self::InternalError(_) | Self::Unauthorized => None,
            Self::BadRequest(v)
            | Self::InvalidValue(v)
            | Self::InvalidFilter(v)
            | Self::PayloadTooLarge(v)
            | Self::NotAcceptable(v) => Some(v.to_owned()),
            _ => Some(self.to_string()),
        }
    }

    /// The variant's name as a metrics label.
    pub fn kind(&self) -> &'static str {
        match &self {
            Self::InternalError(_) => "internal_error",
            Self::MethodNotImplemented => "not_implemented",
            Self::Conflict => "conflict",
            Self::Unauthorized => "unauthorized",
            Self::NotFound(_) => "not_found",
            Self::BadRequest(_) => "bad_request",
            Self::InvalidValue(_) => "invalid_value",
            Self::InvalidFilter(_) => "invalid_filter",
            Self::PayloadTooLarge(_) => "too_large",
            Self::NotAcceptable(_) => "not_acceptable",
        }
    }

    pub fn scim_type(&self) -> Option<&'static str> {
        match &self {
            Self::InvalidValue(_) => Some("invalidValue"),
            Self::InvalidFilter(_) => Some("invalidFilter"),
            Self::PayloadTooLarge(_) => Some("tooLarge"),
            _ => None,
        }
    }

    /// The HTTP status this error answers with.
    ///
    /// `http`'s `StatusCode` rather than actix's. This is what a `ResourceService` returns, so it
    /// has to hold for a backend that is not actix-web - and actix-web 4 is on `http` 0.2 while the
    /// rest of the ecosystem, axum and hyper and tower, is on 1. The HTTP layer converts.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        match &self {
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MethodNotImplemented => StatusCode::NOT_IMPLEMENTED,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) | Self::InvalidValue(_) | Self::InvalidFilter(_) => {
                StatusCode::BAD_REQUEST
            }
            Self::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::NotAcceptable(_) => StatusCode::NOT_ACCEPTABLE,
        }
    }

    /// This error as the document RFC 7644 section 3.12 puts on the wire.
    pub fn to_scim_error(&self) -> crate::models::Error {
        crate::models::Error::new(self.status())
            .with_scim_type(self.scim_type())
            .with_detail(self.get_detail())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
