//! Turns a panicking Job handler into a failed run. Natively the release
//! profile keeps unwinding, so the panic stops at this future and the Job
//! is retried like any other failure. On Workers (`panic = "abort"`) the
//! isolate dies instead and the Job is re-claimed when its lock expires.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::{Error, Result};

type Run<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// Polls the handler's future; a panic becomes
/// `Error::Platform("job handler panicked: <message>")`.
pub(crate) struct CatchPanic<'a>(pub(crate) Run<'a>);

impl Future for CatchPanic<'_> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        let fut = self.0.as_mut();
        // Once it panicked the future is never polled again: we return Ready.
        match catch_unwind(AssertUnwindSafe(|| fut.poll(cx))) {
            Ok(poll) => poll,
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                Poll::Ready(Err(Error::Platform(format!("job handler panicked: {msg}"))))
            }
        }
    }
}
