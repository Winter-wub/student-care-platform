# Channels: LINE and Telegram

Setting up the real Channels. The Fake Channel needs only `FAKE_SECRET`.
Back to the [README](../../README.md).

## Mini App (LIFF and Telegram Web App)

The Announcements Module sends Mini App buttons, so a Deployment with it
needs `LINE_LIFF_ID` and `TELEGRAM_WEB_APP_URL` or `build()` refuses to
start. Both point at the Deployment's HTTPS origin: the LIFF app's endpoint
URL (scope `openid`) and BotFather's Web App URL. `/` forwards to the Mini
App page `/app`; an App Session cookie lasts one hour; `/notice` is public.
The Mini App still needs a manual check inside real LIFF and Telegram
webviews before a release (the e2e walk opens it with the Fake Channel's
token only).

## LINE

Create a LINE Official Account with a Messaging API channel (channel
secret and a long-lived access token) and a LIFF app. Set the webhook URL to
`https://<host>/webhook/line`. Secrets: `LINE_CHANNEL_SECRET`,
`LINE_ACCESS_TOKEN`, `LINE_LIFF_ID`. Rich menu images are uploaded by the
adapter when it sets a Menu.

#### Telegram

```rust
let tg = TelegramChannel::new(
    TelegramConfig::new(bot_token, webhook_secret, web_app_url), // "" = no Web App
    services.http.clone(),
)
.with_clock(services.clock.clone());          // needed for the Mini App Capability (initData freshness)
let d = Deployment::builder() /* ... */ .channel(tg.clone()).build()?;
tg.with_menu(&d.menu_for(&d.config().default_locale)); // clones share the map
```

Telegram sends a Menu command back as text (`/news_latest`). The adapter maps
it to `Inbound::Action` through the map from `with_menu` (the Deployment's
unlocked Menu) and, per account, the last `set_menu` call. That overlay is in
memory, so on Workers it is per isolate: `with_menu` is the reliable source.
Mapping a command for an account without a Link is safe, as the Kernel never
dispatches its Actions to a Module. The adapter does not pace sends: a 429
becomes `Error::RateLimited` with Telegram's `retry_after`, and bulk sends are
paced by the Job runner.

Before a real webhook works: create the bot with BotFather (token), then call
`setWebhook` with `url = https://<host>/webhook/telegram`, `secret_token =
webhook_secret` and `allowed_updates = ["message", "callback_query",
"my_chat_member"]`. For the Mini App, set the Web App URL (BotFather
`/newapp` or the menu button set by `set_menu`) to the Deployment's HTTPS
origin.
