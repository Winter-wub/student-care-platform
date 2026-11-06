//! [`Smtp`]: email over SMTP with lettre's async (tokio) transport, for a
//! customer's own mail relay. The Resend sender is the Core's
//! [`harpseal::ResendEmail`].

use async_trait::async_trait;
use harpseal::{Email, EmailSender, Error, Result};
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Email over SMTP. The URL follows lettre's format:
/// `smtp://host:25` (plain, STARTTLS if offered), `smtp://host:587?tls=required`
/// (STARTTLS required), `smtps://user:pass@host:465` (implicit TLS). The
/// password belongs in the environment, never in TOML.
pub struct Smtp {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

fn platform(e: impl core::fmt::Display) -> Error {
    Error::Platform(e.to_string())
}

impl Smtp {
    /// A sender for `url`, sending as `from` (`"Name <addr@domain>"` or a bare
    /// address). Nothing connects until the first [`EmailSender::send`].
    pub fn new(url: &str, from: &str) -> Result<Self> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| Error::Invalid(format!("SMTP URL: {e}")))?
            .build();
        let from = from
            .parse()
            .map_err(|e| Error::Invalid(format!("email sender {from:?}: {e}")))?;
        Ok(Self { transport, from })
    }

    fn message(&self, email: Email) -> Result<Message> {
        let to: Mailbox = email
            .to
            .parse()
            .map_err(|e| Error::Invalid(format!("email recipient {:?}: {e}", email.to)))?;
        let b = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(email.subject);
        let m = match email.html {
            Some(html) => b.multipart(MultiPart::alternative_plain_html(email.text, html)),
            None => b.singlepart(SinglePart::plain(email.text)),
        };
        m.map_err(platform)
    }
}

#[async_trait]
impl EmailSender for Smtp {
    async fn send(&self, email: Email) -> Result<()> {
        let msg = self.message(email)?;
        self.transport.send(msg).await.map(|_| ()).map_err(platform)
    }
}
