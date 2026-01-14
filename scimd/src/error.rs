use actix_web::{HttpResponse, ResponseError, http::StatusCode, http::header};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("an unspecified internal error occurred: {0}")]
    InternalError(#[from] anyhow::Error),

    #[error("Method not implemented")]
    MethodNotImplemented,

    #[error("Conflict")]
    Conflict,

    #[error("Resouce {0} not found")]
    NotFound(uuid::Uuid),
}

impl Error {
    fn get_detail(&self) -> Option<String> {
        match &self {
            Error::InternalError(_) => None,
            _ => Some(self.to_string()),
        }
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match &self {
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MethodNotImplemented => StatusCode::NOT_IMPLEMENTED,
            Self::Conflict => StatusCode::CONFLICT,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let status = self.status_code();
        HttpResponse::build(status)
            .append_header((header::CONTENT_TYPE, "application/scim+json"))
            .json(crate::models::Error::new(status).with_detail(self.get_detail()))
    }
}

// Short hand alias, which allows you to use just Result<T>
pub type Result<T> = std::result::Result<T, Error>;
