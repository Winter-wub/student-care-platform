# Channels: LINE and Telegram

Setting up the real Channels. The Fake Channel needs only `FAKE_SECRET`.
Back to the [README](https://github.com/Winter-wub/student-care-platform#readme).

## Writing an adapter

Build `Inbound` events with the constructors (`Inbound::text`, `action`,
`file`, `follow`, `blocked`, `unblocked`), then stamp the platform's id with
`with_event_id` (and `with_redelivery`); match them with `..`. Each variant
is `#[non_exhaustive]`, so struct literals do not compile outside the Core
and a field added later breaks no adapter. A Channel that honours retry
keys reports a request the platform answered as already accepted (LINE 409)
in `SendReceipt::replayed_units` (`add_replayed_unit`).

## Mini App (LIFF and Telegram Web App)

The Announcements Module sends Mini App buttons, so a Deployment with it
needs `LINE_LIFF_ID` and `TELEGRAM_WEB_APP_URL` or `build()` refuses to
start. Both point at the Deployment's HTTPS origin: the LIFF app's endpoint
URL (scope `openid`) and BotFather's Web App URL. `/` forwards to the Mini
App page `/app`; an App Session cookie lasts one hour; `/notice` is public.
Opened outside both chat apps (Staff typing the bare domain), the page
shows links to `/notice` and `/admin/login` instead of starting LINE Login.
A LINE MINI App is a LIFF app too: build the `LineConfig` with
`.with_launch_base(LINE_MINI_APP_BASE)` so buttons open
`https://miniapp.line.me/{liffId}`. On Telegram Web (web.telegram.org) the
page runs in a cross-site iframe and its cookie is
`SameSite=None; Secure; Partitioned`; a browser that blocks third-party
cookies without supporting partitioned ones ends at "open from the chat".
The Mini App still needs a manual check inside real LIFF and Telegram
webviews before a release (the e2e walk opens it with the Fake Channel's
token only).

## LINE

Create a LINE Official Account with a Messaging API channel (channel
secret and a long-lived access token) and a LIFF app. Set the webhook URL to
`https://<host>/webhook/line`. Secrets: `LINE_CHANNEL_SECRET`,
`LINE_ACCESS_TOKEN`, `LINE_LIFF_ID`. Rich menu images are uploaded by the
adapter when it sets a Menu.

Each Menu becomes one rich menu, created on first use and reused by name:
`harpseal-<key>-<hash>`, where `<hash>` covers the rendered rich menu and its
image and `<key>` says which Menu it is: `locked` for the locked Menu, or the
key the unlocked Menu was registered under. Register the unlocked Menus once
the Deployment is built (clones of the channel share the registration):

```rust
let line = LineChannel::new(line_config, services.http.clone());
let d = Deployment::builder() /* ... */ .channel(line.clone()).build()?;
line.with_menus([("th", d.menu_for(&Locale::th())), ("en", d.menu_for(&Locale::en()))])?;
```

A channel holds at most 1000 rich menus, and every change to a Menu or an image
makes a new one. After a `set_menu` that had to look the rich menu up on LINE
(a cold cache), the adapter deletes, 20 at most per call, the other versions
under the same key and any duplicate of the one it linked (two isolates can
create the same rich menu at once; all keep the smallest id). Rich menus with
another or no key (`harpseal-<hash>`: an unregistered Menu, e.g. one whose
labels a setting changed), and those not named `harpseal-*`, are never
deleted. LINE cannot list the accounts linked to a rich menu, so an account
still on a deleted version sees the default rich menu set in the LINE Official
Account Manager (or none) until its next `set_menu`. A rich menu deleted in the
console is recreated on the next `set_menu` that links it.

A 403 on a send to one account makes the Link `unreachable` only when LINE's
message is about that account (`Failed to send messages`, `Not found`). Any
other 403, such as `Access to this API is not available for your account`
(plan or permission), is a platform error, and every Link stays as it is.

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
