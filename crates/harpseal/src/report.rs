//! The error-reporting hook of the web handlers: a handler that answers a
//! generic error page attaches an [`ErrorReport`] to the response, and the
//! runtime crate (which owns the request loop) writes it to its log. Only
//! the area, the [`Error`] kind and the route pattern travel, never the
//! message: an adapter's or the database's message may echo a token, an ID
//! token or a Student's data.
//!
//! A Module's back-office handler gets a report for free from
//! [`crate::ui::service_error`] (a 500 there names the Module and its
//! route); any other handler, e.g. a Mini App page, attaches one with
//! [`ErrorReport::attach`].

use axum::response::Response;

use crate::Error;

/// A failure a handler answered with a generic error page, in a response
/// extension. Read it with [`ErrorReport::of`]; `Display` gives one log
/// line (`"<area>: <kind>"`, then `" at /admin<route>"` when the route is
/// known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ErrorReport {
    /// Where it happened, e.g. `app.exchange`, or the Module's name.
    pub area: &'static str,
    /// The [`Error`] kind ([`Error::kind`]), e.g. `db`.
    pub kind: &'static str,
    /// The back-office route as the Module registered it, under `/admin`
    /// (e.g. `/fees/{id}/cancel`): the pattern, never the path's values.
    /// `None` outside a Module's back-office route.
    pub route: Option<&'static str>,
}

impl ErrorReport {
    /// The report a handler attached to `response`, if any.
    pub fn of<B>(response: &http::Response<B>) -> Option<&ErrorReport> {
        response.extensions().get::<ErrorReport>()
    }

    /// Attaches a report of `e` in `area` to `response`, so the runtime
    /// logs it as it logs the Core's own (`area` is a fixed name such as
    /// `fees.student`; never put a Student's data in it):
    ///
    /// ```
    /// use harpseal::web::axum::http::StatusCode;
    /// use harpseal::web::axum::response::{IntoResponse, Response};
    /// use harpseal::{Error, ErrorReport};
    ///
    /// fn failed(e: &Error) -> Response {
    ///     let page = StatusCode::INTERNAL_SERVER_ERROR.into_response();
    ///     ErrorReport::attach(page, "fees.student", e)
    /// }
    /// let r = failed(&Error::Db("no such table".into()));
    /// assert_eq!(ErrorReport::of(&r).unwrap().to_string(), "fees.student: db");
    /// ```
    pub fn attach(response: Response, area: &'static str, e: &Error) -> Response {
        Self::attach_at(response, area, None, e)
    }

    /// [`ErrorReport::attach`] with the back-office route.
    pub(crate) fn attach_at(
        mut response: Response,
        area: &'static str,
        route: Option<&'static str>,
        e: &Error,
    ) -> Response {
        let kind = e.kind();
        let report = ErrorReport { area, kind, route };
        response.extensions_mut().insert(report);
        response
    }

    /// The line a runtime logs for `response`: `error: <report> (request
    /// <x-request-id>)`, or `None` when no report is attached. Both
    /// runtimes write this line (stderr natively, `console.error` on
    /// Workers).
    pub fn log_line<B>(response: &http::Response<B>) -> Option<String> {
        let report = Self::of(response)?;
        let id = response
            .headers()
            .get(crate::audit::REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-");
        Some(format!("error: {report} (request {id})"))
    }
}

impl core::fmt::Display for ErrorReport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: {}", self.area, self.kind)?;
        if let Some(route) = self.route {
            write!(f, " at /admin{route}")?;
        }
        Ok(())
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

    #[test]
    fn the_log_line_names_the_route_and_the_request() {
        let e = Error::Db("UNIQUE failed: 68010001".into());
        let mut r = ().into_response();
        let id = "0123abcd".parse().unwrap();
        r.headers_mut().insert(crate::audit::REQUEST_ID_HEADER, id);
        let r = ErrorReport::attach_at(r, "fees", Some("/fees/{id}/cancel"), &e);
        let line = ErrorReport::log_line(&r).unwrap();
        assert_eq!(
            line,
            "error: fees: db at /admin/fees/{id}/cancel (request 0123abcd)"
        );
        assert!(!line.contains("68010001"));
        assert_eq!(ErrorReport::log_line(&().into_response()), None);
    }
}
