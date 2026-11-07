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
`harpseal-<key>-<generation>-<hash>`. `<hash>` covers the rendered rich menu
and its image; `<key>` says which Menu it is and comes from the Menu itself
(`Menu::key`): `locked` for the locked Menu, and for an unlocked Menu the
Menu key of its Locale (`MenuKey::for_locale`: the tag in lowercase without
`-` or `_`, so `th`, `en`, `zh-Hant` -> `zhhant`). The Kernel's Menus
(`Deployment::menu_for`, `menu_with`, and the chat flow's Menu with `setting`
overrides) always carry that key, so a back-office label edit keeps it.
`<generation>` comes from `LineConfig::with_menu_generation` (default 1).
Nothing needs registering:

```rust
let line = LineChannel::new(line_config.with_menu_generation(1), services.http.clone());
```

A Menu built without a key (`Menu::new`, outside the Kernel) is named
`harpseal-<hash>` and is never cleaned up. `LineChannel::with_menus` and the
hash-based registration are gone (#64).

A channel holds at most 1000 rich menus, and every change to a Menu or an image
makes a new one. After a `set_menu` that had to look the rich menu up on LINE
(a cold cache), the adapter deletes, 5 at most per call (LINE allows 100
deletes an hour; the rest go on later cold calls):

- rich menus under the same key with a **lower** generation;
- duplicates of the one it linked (two isolates can create the same rich menu
  at once; all keep the smallest id).

The same key and generation with another hash is left alone: during a gradual
deploy, or with a staging Deployment on the production token, two versions
link their own rich menus, and deleting each other's would cycle. So cleanup is
clean only while one generation runs on a channel at a time. Bump the
generation when a Menu change must win; changes within one generation only
leak (the old rich menus stay until deleted by hand in the console). Higher
generations, other keys, rich menus without a key (`harpseal-<hash>`) and
names not made by the adapter (e.g. `harpseal-th-backup`) are never deleted.

Limits to know:

- **Label edits within one generation leak.** A back-office label edit keeps
  the Menu key, but makes a new rich menu under the same generation, which is
  left alone like any same-generation version. The old ones go with the next
  generation bump.
- **Accounts on a deleted rich menu.** LINE cannot list the accounts linked to
  a rich menu. An account still on a deleted older generation sees the default
  rich menu set in the LINE Official Account Manager (or none) until its next
  `set_menu`. The same holds after a cold race: an account that the losing
  isolate linked to its twin loses its menu when that twin is deleted.
- A rich menu deleted in the console is recreated on the next `set_menu` that
  links it (once per call; a second 404 is a platform error).

Blocks arrive as `unfollow` events, which make the Link `unreachable`. LINE
documents a push to a user who blocked or never added the account as a 200, an
unknown user id as a 400, and `Access to this API is not available for your
account` as the 403 for a missing permission. So a 403 on a send is a platform
error, and every Link stays as it is. As a defensive fallback, two 403 bodies
that LINE does not document for a push (`Failed to send messages`,
`Not found`) still make the Link `unreachable`; verify them on a live account.

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
