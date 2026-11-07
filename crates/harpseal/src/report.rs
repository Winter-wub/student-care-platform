//! The error-reporting hook of the web handlers: a handler that answers a
//! generic error page attaches an [`ErrorReport`] to the response, and the
//! runtime crate (which owns the request loop) writes it to its log. Only
//! the area and the [`Error`] kind travel, never the message: an adapter's
//! or the database's message may echo a token, an ID token or a Student's
//! data.

use axum::response::Response;

use crate::Error;

/// A failure a handler answered with a generic error page, in a response
/// extension. Read it with [`ErrorReport::of`]; `Display` gives one log
/// line (`"<area>: <kind>"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ErrorReport {
    /// Where it happened, e.g. `app.exchange`.
    pub area: &'static str,
    /// The [`Error`] kind ([`Error::kind`]), e.g. `db`.
    pub kind: &'static str,
}

impl ErrorReport {
    /// The report a handler attached to `response`, if any.
    pub fn of<B>(response: &http::Response<B>) -> Option<&ErrorReport> {
        response.extensions().get::<ErrorReport>()
    }

    /// Attaches a report of `e` in `area` to `response`.
    pub(crate) fn attach(mut response: Response, area: &'static str, e: &Error) -> Response {
        let kind = e.kind();
        response.extensions_mut().insert(ErrorReport { area, kind });
        response
    }
}

impl core::fmt::Display for ErrorReport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.area, self.kind)
    }
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;

    use super::ErrorReport;
    use crate::Error;

    #[test]
    fn a_report_carries_the_kind_and_never_the_message() {
        let e = Error::Platform("id_token=eyJsecret".into());
        let r = ErrorReport::attach(().into_response(), "app.exchange", &e);
        let report = ErrorReport::of(&r).copied().unwrap();
        assert_eq!(report.to_string(), "app.exchange: platform");
        assert!(!format!("{report:?}").contains("eyJ"));
    }
}
