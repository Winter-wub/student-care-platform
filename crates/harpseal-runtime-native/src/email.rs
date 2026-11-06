//! [`Smtp`]: email over SMTP with lettre's async (tokio) transport, for a
//! customer's own mail relay. The Resend sender is the Core's
//! [`harpseal::ResendEmail`].

use async_trait::async_trait;
use harpseal::{Email, EmailSender, Error, Result};
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

/// Email over SMTP. The URL follows lettre's `from_url` format:
///
/// - `smtp://host:25`: **unencrypted**, never upgraded;
/// - `smtp://host:25?tls=opportunistic`: STARTTLS when the server offers it;
/// - `smtp://user:pass@host:587?tls=required`: STARTTLS or fail;
/// - `smtps://user:pass@host:465`: implicit TLS.
///
/// Credentials in a plain `smtp://` URL without `tls=required` or
/// `tls=opportunistic` are
/// refused: they would cross the network in clear text. The password belongs
/// in the environment, never in TOML.
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
        refuse_clear_text_credentials(url)?;
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

/// `smtp://user:pass@host` with no `tls=` would send the password unencrypted.
fn refuse_clear_text_credentials(url: &str) -> Result<()> {
    let Some(rest) = url.strip_prefix("smtp://") else {
        return Ok(());
    };
    let (before_query, query) = rest.split_once('?').unwrap_or((rest, ""));
    let authority = before_query.split('/').next().unwrap_or_default();
    let has_tls = query
        .split('&')
        .any(|kv| kv == "tls=required" || kv == "tls=opportunistic");
    if authority.contains('@') && !has_tls {
        return Err(Error::Invalid(
            "SMTP URL has credentials but no TLS: use smtps://, or add \
             ?tls=required (or ?tls=opportunistic) to the smtp:// URL"
                .into(),
        ));
    }
    Ok(())
}

#[async_trait]
impl EmailSender for Smtp {
    async fn send(&self, email: Email) -> Result<()> {
        let msg = self.message(email)?;
        self.transport.send(msg).await.map(|_| ()).map_err(platform)
    }
}
