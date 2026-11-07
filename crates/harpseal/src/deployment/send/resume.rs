//! [`SendOptions`]: the idempotency key of a batch and where a resume
//! starts (#62).

use crate::{Error, ResumeToken};

/// How [`crate::Deployment::send_batch_with`] sends a batch: its
/// idempotency key and, on a resume, where the last attempt stopped.
///
/// ```
/// # use harpseal::{Error, SendOptions};
/// let first = SendOptions::new().key("job-7/u1");
/// // The first attempt stopped part-way: resume where it left off.
/// let stopped = Error::partial_send(2, 5, Error::RateLimited { retry_after_secs: 30 });
/// let again = first.clone().resume_after(&stopped);
/// assert_eq!(again.first_message(), 2);
/// ```
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SendOptions {
    pub(super) key: Option<String>,
    pub(super) first: usize,
    pub(super) resume: Option<ResumeToken>,
}

impl SendOptions {
    /// A fresh batch with a key of its own.
    pub fn new() -> Self {
        Self::default()
    }

    /// The batch's idempotency key (e.g. `<job id>/<account>`): a retry
    /// with the same key, account and Messages is the same send (see
    /// [`crate::Deployment::send_batch_with_key`]), and its
    /// `outbound_message` rows are found again, so nothing is metered
    /// twice.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Resumes after `err`: when it is an [`Error::PartialSend`], the send
    /// starts at Message `sent` of the batch, hands the Channel its resume
    /// token and, when these options have no key, takes the batch key the
    /// Kernel used ([`Error::batch_key`]; an unkeyed send's fresh key), so
    /// the resume finds the batch's rows and retry keys. Any other error
    /// leaves the options as they are (a retry of the whole batch, e.g.
    /// after `RateLimited` with nothing sent).
    pub fn resume_after(mut self, err: &Error) -> Self {
        if let Error::PartialSend {
            sent, resume, key, ..
        } = err
        {
            self.first = *sent;
            self.resume = resume.clone();
            if self.key.is_none() {
                self.key = key.clone();
            }
        }
        self
    }

    /// Starts the send at Message `first` of the batch (the Messages before
    /// it went out and were recorded), without a resume token.
    pub fn from_message(mut self, first: usize) -> Self {
        self.first = first;
        self.resume = None;
        self
    }

    /// The index of the first Message this send covers.
    pub fn first_message(&self) -> usize {
        self.first
    }

    /// The Channel's resume token this send hands back, if any.
    pub fn resume_token(&self) -> Option<&ResumeToken> {
        self.resume.as_ref()
    }
}
