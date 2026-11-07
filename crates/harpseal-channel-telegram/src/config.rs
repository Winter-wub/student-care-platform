//! The secrets and settings of one Telegram bot.

/// Default text of the Web App menu button (Telegram refuses an empty one,
/// <https://core.telegram.org/bots/api#menubuttonwebapp>).
pub const DEFAULT_MENU_BUTTON_TEXT: &str = "เปิดแอป";

/// Chats whose commands one Job run re-sets after a Menu label save, by
/// default: two calls each (`setMyCommands`, `setChatMenuButton`), so 40
/// subrequests, under the 50 a Cloudflare Workers invocation gets on the
/// free plan (1,000 on paid plans; raise it there with
/// [`TelegramConfig::with_menu_relink_chunk`]).
pub const DEFAULT_MENU_RELINK_CHUNK: usize = 20;

/// Secrets of one Telegram bot. Read from the environment or Worker
/// secrets, never from TOML. `Debug` hides the token and the secret.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TelegramConfig {
    /// Bot token from BotFather.
    pub bot_token: String,
    /// Checked against `X-Telegram-Bot-Api-Secret-Token`; give the same
    /// value as `secret_token` to `setWebhook`.
    pub webhook_secret: String,
    /// Web App URL; empty when the Deployment has no Mini App on Telegram.
    pub web_app_url: String,
    /// Text of the chat's menu button when it opens the Web App.
    pub menu_button_text: String,
    /// Chats re-set per Job run after a Menu label save
    /// ([`DEFAULT_MENU_RELINK_CHUNK`]; 0 counts as 1).
    pub menu_relink_chunk: usize,
}

impl TelegramConfig {
    /// A configuration; pass an empty `web_app_url` for no Mini App.
    pub fn new(
        bot_token: impl Into<String>,
        webhook_secret: impl Into<String>,
        web_app_url: impl Into<String>,
    ) -> Self {
        Self {
            bot_token: bot_token.into(),
            webhook_secret: webhook_secret.into(),
            web_app_url: web_app_url.into(),
            menu_button_text: DEFAULT_MENU_BUTTON_TEXT.into(),
            menu_relink_chunk: DEFAULT_MENU_RELINK_CHUNK,
        }
    }

    /// Another text for the Web App menu button; an empty one keeps the
    /// default.
    pub fn with_menu_button_text(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        if !text.is_empty() {
            self.menu_button_text = text;
        }
        self
    }
}

impl TelegramConfig {
    /// Chats re-set per Job run after a Menu label save (two calls each):
    /// keep `2 * n` plus a few under the runtime's subrequest limit.
    pub fn with_menu_relink_chunk(mut self, n: usize) -> Self {
        self.menu_relink_chunk = n.max(1);
        self
    }
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self::new("", "", "")
    }
}

impl core::fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let hidden = |s: &str| {
            if s.is_empty() {
                "<empty>"
            } else {
                "<redacted>"
            }
        };
        f.debug_struct("TelegramConfig")
            .field("bot_token", &hidden(&self.bot_token))
            .field("webhook_secret", &hidden(&self.webhook_secret))
            .field("web_app_url", &self.web_app_url)
            .field("menu_button_text", &self.menu_button_text)
            .field("menu_relink_chunk", &self.menu_relink_chunk)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_secrets() {
        let cfg = TelegramConfig::new("8000000001:AAFsealBaySecretToken", "seal-bay-hook", "");
        let text = format!("{cfg:?}");
        assert!(
            !text.contains("AAFsealBay") && !text.contains("seal-bay-hook"),
            "{text}"
        );
        assert!(text.contains("<redacted>"));
    }

    #[test]
    fn the_default_button_text_is_not_empty() {
        assert_eq!(
            TelegramConfig::default().menu_button_text,
            DEFAULT_MENU_BUTTON_TEXT
        );
        let cfg = TelegramConfig::default().with_menu_button_text("");
        assert_eq!(cfg.menu_button_text, DEFAULT_MENU_BUTTON_TEXT);
    }
}
