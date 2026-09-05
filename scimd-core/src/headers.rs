//! The two request-header checks every SCIM request passes, expressed against header *bytes* so
//! that an adapter for any framework can run them.
//!
//! Content negotiation belongs here rather than in a router-level guard: a guard that does not
//! match makes the route disappear, so a client with the wrong `Accept` would be told the endpoint
//! does not exist instead of that its `Accept` is the problem.

use scimd_types::{Error, Result};

const AUTH_TOKEN_PREFIX: &str = "Bearer ";

/// What scimd sends. RFC 7644 section 3.1.
pub const SCIM_MEDIA_TYPE: &str = "application/scim+json";
/// Which that same section allows a SCIM server to answer with as well.
pub const JSON_MEDIA_TYPE: &str = "application/json";

/// Whether the `Authorization` header carries the bearer token this server expects.
///
/// `expected` of `None` means the server has no token, and then a client that *presents* one is
/// still refused - a token nobody asked for is a sign the client thinks it is talking to something
/// else. A header that is not valid text reads as absent, since it cannot be the token either way.
///
/// # Errors
///
/// [`Error::Unauthorized`], with no detail: an unauthenticated client learns nothing from the reply
/// beyond that it is unauthenticated.
pub fn validate_token(authorization: Option<&[u8]>, expected: Option<&str>) -> Result<()> {
    let presented = authorization
        .and_then(|v| std::str::from_utf8(v).ok())
        .map(|t| t.strip_prefix(AUTH_TOKEN_PREFIX).ok_or(Error::Unauthorized))
        .transpose()?;

    if presented == expected {
        Ok(())
    } else {
        Err(Error::Unauthorized)
    }
}

/// Whether the client will take what we are going to send.
///
/// Per RFC 7231 section 5.3.2 an absent or empty `Accept` means the client accepts anything, and
/// `*/*` and `application/*` both cover [`SCIM_MEDIA_TYPE`]. Media types are compared
/// case-insensitively, as that section requires.
///
/// # Errors
///
/// [`Error::NotAcceptable`] if the header is not valid text, or names no type this server sends.
pub fn validate_accept(accept: Option<&[u8]>) -> Result<()> {
    let Some(accept) = accept else {
        return Ok(());
    };
    let accept = std::str::from_utf8(accept)
        .map_err(|_| Error::NotAcceptable("the Accept header is not valid text".into()))?;
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
        Err(Error::NotAcceptable(format!(
            "this endpoint answers {SCIM_MEDIA_TYPE}, which '{accept}' does not accept"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(value: &str) -> Option<&[u8]> {
        Some(value.as_bytes())
    }

    #[test]
    fn a_token_is_required_only_when_the_server_has_one() {
        assert!(validate_token(None, None).is_ok());
        assert!(validate_token(h("Bearer s3cret"), Some("s3cret")).is_ok());

        // No token where one is expected, and a token where none is.
        assert!(validate_token(None, Some("s3cret")).is_err());
        assert!(validate_token(h("Bearer s3cret"), None).is_err());
        assert!(validate_token(h("Bearer wrong"), Some("s3cret")).is_err());
    }

    #[test]
    fn an_authorization_that_is_not_a_bearer_token_is_refused() {
        assert!(validate_token(h("Basic aGk6dGhlcmU="), Some("s3cret")).is_err());
        // The scheme is case-sensitive per RFC 6750, and so is the prefix here.
        assert!(validate_token(h("bearer s3cret"), Some("s3cret")).is_err());
    }

    #[test]
    fn an_unreadable_authorization_header_reads_as_absent() {
        assert!(validate_token(Some(&[0xff, 0xfe]), None).is_ok());
        assert!(validate_token(Some(&[0xff, 0xfe]), Some("s3cret")).is_err());
    }

    #[test]
    fn an_absent_or_wildcard_accept_takes_what_we_send() {
        for accept in [None, h(""), h("   "), h("*/*"), h("application/*")] {
            assert!(validate_accept(accept).is_ok(), "{accept:?}");
        }
    }

    #[test]
    fn accept_matches_case_insensitively_and_ignores_parameters() {
        for accept in [
            h("application/scim+json"),
            h("APPLICATION/SCIM+JSON"),
            h("application/scim+json; charset=utf-8"),
            h("application/json"),
            h("text/html, application/scim+json;q=0.9"),
        ] {
            assert!(validate_accept(accept).is_ok(), "{accept:?}");
        }
    }

    #[test]
    fn an_accept_we_cannot_satisfy_is_refused() {
        assert!(validate_accept(h("text/html")).is_err());
        assert!(validate_accept(h("application/xml, text/plain")).is_err());
        // A near miss: `scim+json` is not a type of its own.
        assert!(validate_accept(h("scim+json")).is_err());
        assert!(validate_accept(Some(&[0xff, 0xfe])).is_err());
    }
}
