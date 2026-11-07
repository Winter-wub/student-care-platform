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

After a Menu label save the Kernel calls `Channel::relink_menu` with a
chunk of `Channel::relink_chunk` accounts (default 20) and, once every
account is done, `Channel::retire_menus` with the older revisions. The
defaults call `set_menu` per account, passing over an account that answers
`Error::Unreachable` (`MenuRelinked::skipped`), and retire nothing. An
adapter whose Menu is a shared platform object retires only versions that
carry the Deployment's salt (`Menu::salt`), and answers
`MenuRetire::Pending` until the platform confirms nobody is left on them.

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

Every launch asks the platform again who is there (#86). A page load that
does not come from one of the Deployment's own pages (opened from the chat,
LIFF's redirect, Telegram Web's iframe, a Card's deep link, a typed URL or
a bookmark) drops the App Session, row and cookie, before anything is read
and shows the entry page, which posts the current LIFF ID token or
Telegram `initData` again. So the next person on a shared computer (a lab
PC on Telegram Web, LIFF in an external browser) gets their own page, or
"not linked", not the previous Student's. Links, forms and HTMX inside the
Mini App continue the launch at no extra cost. The browser says which is
which with `Sec-Fetch-Site` (only `same-origin` continues); an older
webview without it continues only when its `Referer` is the request's own
host (Mini App pages send `Referrer-Policy: same-origin`).

What the launch check cannot see, and what covers it:

- **Back/forward, reload, a reopened tab**: browsers resend the original
  `same-origin`, or restore the page from the bfcache with no request. An
  App Session ends after `[mini_app] idle_minutes` without a request
  (default 15, allowed 1 to 60; each request extends it, never past one
  hour from the exchange), and a page restored from the bfcache reloads
  through the entry page. Within those minutes, Back can still show the
  previous Student's page: lower `idle_minutes` for lab PCs, and the
  Student can tap "sign out".
- **Two tabs share one cookie**: each session page checks a stamp of its
  session against the tab's `sessionStorage` before it is painted; a page
  of another tab's session hides itself and signs the tab in again.
- Any site can make the visitor sign in again (a cross-site link or image
  is a new launch); it cannot sign them in as anyone.

The cost: one more round trip per launch (entry page, exchange, redirect,
as for a launch without a cookie), and on LINE one call to LINE's verify
endpoint per launch. Only refused tokens count toward the exchange's limit
(30 a minute per IP group), so a class behind one campus Wi-Fi address can
all open a Card at once. The home has a "sign out" button
(`POST /app/sign-out`) that ends the App Session at once and closes the
Mini App inside LINE and Telegram. This protects the next honest user, not
against someone at the keyboard who wants the previous session on purpose.
The Mini App still needs a manual check inside real LIFF and Telegram
webviews before a release (the e2e walk opens it with the Fake Channel's
token only). A Deployment's own Module adds pages under `/app/` and links on
the home: see "A Module's Mini App page" in [deployment.md](deployment.md).

## LINE

Create a LINE Official Account with a Messaging API channel (channel
secret and a long-lived access token) and a LIFF app. Set the webhook URL to
`https://<host>/webhook/line`. Secrets: `LINE_CHANNEL_SECRET`,
`LINE_ACCESS_TOKEN`, `LINE_LIFF_ID`. Rich menu images are uploaded by the
adapter when it sets a Menu.

Each Menu becomes one rich menu, created on first use and reused by name:
`harpseal-<key>-<generation>-<hash>`. `<hash>` covers the rendered rich menu
and its image; `<key>` says which Menu it is and comes from the Menu itself
(`Menu::key`): `locked` for the locked Menu (reserved: no unlocked Menu can
take it), and for an unlocked Menu the Menu key of its *effective* Locale
(`Catalog::effective`: the Student's Locale when the catalog has texts in it,
else the default Locale; then `MenuKey::for_locale`: the tag in lowercase
without `-` or `_`, so `th`, `en`, `zh-Hant` -> `zhhant`). A Student whose
roster cell says `TH`, `en-US` or `locked` reads the default Locale's texts,
and gets its Menu and key. `build()` refuses a catalog Locale that maps to no
key or to the key of another (`BuildError::MenuKey`), so the Kernel's Menus
(`Deployment::menu_for`, `menu_with`, and the chat flow's Menu with `setting`
overrides) always carry a key for every catalog Locale, and a back-office
label edit keeps it.
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
generation when a Menu change must win; other changes within one generation
(a new image, new entries) only leak (the old rich menus stay until deleted by
hand in the console). A Menu label save is retired by the re-link below. Higher
generations, other keys, rich menus without a key (`harpseal-<hash>`) and
names not made by the adapter (e.g. `harpseal-th-backup`) are never deleted.

Limits to know:

- **A label edit re-links every linked account** (#66). Saving (or
  resetting) a Menu label on the Texts page records the next **Menu
  revision** of each effective Locale whose labels change, and enqueues the
  Kernel Job `kernel.menu.relink` for it. The first save also draws the
  Deployment's **revision salt** (8 random hex digits, stored in the
  database and backed up with it). A revisioned Menu is named
  `harpseal-<key>-<generation>-r<revision>-<salt>-<hash>`; revision 0, the
  Menu shown before the first edit, keeps the old name. The Job bulk-links
  every account with an active Link and an unlocked Menu in that Locale
  (`POST /v2/bot/richmenu/bulk/link`, at most 500 user ids per call, 2,000
  requests a second:
  <https://developers.line.biz/en/reference/messaging-api/#link-rich-menu-to-users>),
  2,500 accounts per Job run (about 8 Workers subrequests). An id that is
  not a LINE user id (`U` + 32 hex) is never sent, and one LINE refuses
  (a 400 naming `userIds[n]`) is dropped and the call sent again, so one
  bad account never stops the others; an account a Channel reports
  unreachable is passed over and its Link marked `unreachable`. Any other
  failure fails the run, and the Job's retry re-links that chunk again;
  nothing is retired yet.
- **The previous revision is retired only after confirmed completion.**
  The bulk link answers 202 and works in the background, so a 2xx proves
  nothing. Once every account was bulk-linked, the adapter sends one
  `POST /v2/bot/richmenu/batch` that moves anyone still on a previous
  revision (`link` from its rich menu to the new one, with a
  `resumeRequestKey`), polls `GET /v2/bot/richmenu/progress/batch` after 5
  minutes, then 10, 20, 40 and every hour, and deletes the previous
  revisions' rich menus only when it says `succeeded`
  (<https://developers.line.biz/en/reference/messaging-api/#batch-control-rich-menus-of-users>).
  Batch control allows **3 requests an hour** (a 429 waits 20 minutes, so
  the cleanup may lag; the new labels do not); progress polls and deletes
  allow **100 an hour** per channel, so one batch costs at most 5 polls in
  its first hour and 1 an hour after
  (<https://developers.line.biz/en/reference/messaging-api/#rate-limits>).
  Each batch is polled by one Job only (the Locale that sent it).
- **Batches never overlap.** A batch still pending is stored per Locale
  and Channel. A save during a run makes a newer revision: the older run
  stops, and the newer one re-links everyone, then takes the pending batch
  over and sends nothing new until LINE reports it `succeeded` (its rich
  menus deleted) or `failed` (nothing deleted); only then does it move
  everyone still on an older revision, so no rich menu is deleted while an
  earlier batch may still move users onto it.
- **Only this Deployment's salted revisions are retired.** They are found
  by exact name (this key, generation and salt, each revision's own
  labels). Revision 0 and Menus without a key carry no salt: a staging
  Deployment on the production token with the default labels shows the
  very same rich menu, so they are never moved or deleted by a retire;
  they go with the next generation bump, as before. The same-generation
  KEEP rule above holds for every other rich menu too: another salt,
  another key, a revision not handed over. A restore draws a new salt, so a copy of
  production restored into staging on the same token never names (or
  retires) rich menus like production; the rich menus named with the old
  salt are left to leak (safe) until a generation bump.
- **Mid-run unlinks.** When a run is done (and its batch succeeded),
  accounts whose Menu was locked since it started (an unlink, a revoke, a
  new Consent) get the locked Menu again, in case the re-link or the batch
  moved them: one `set_menu` each, at most 20 per Job run (under the 50
  Workers subrequests), and an account whose call failed for a passing
  reason (a 429, a 5xx) is tried again in the next run, never skipped.
  Their Actions were never at risk: the Kernel dispatches an Action to a
  Module only for an account with a live Link.
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

A Menu label save re-sets the commands of every linked chat (their
descriptions are the labels): the Kernel Job `kernel.menu.relink` calls
`set_menu` per account (`setMyCommands` and `setChatMenuButton`, two calls
each), 20 chats per Job run by default: 40 subrequests, under the 50 a
Cloudflare Workers invocation gets on the free plan (1,000 on paid plans;
raise it there with `TelegramConfig::with_menu_relink_chunk`). One run per
tick (a minute) re-sets about **1,200 chats an hour** with the default, so a
roster of 5,000 linked chats takes about 4 hours; with a chunk of 450 on a
paid plan (900 calls a run) it is about 27,000 an hour, within Telegram's
30 calls a second. A 429 continues after Telegram's `retry_after`; a chat
that blocked the bot or was deleted (403), or that the Bot API no longer
knows (`400 Bad Request: chat not found`), is passed over and its Link
marked `unreachable`.
Commands are per chat, not a shared object, so there is nothing to
retire.

Before a real webhook works: create the bot with BotFather (token), then call
`setWebhook` with `url = https://<host>/webhook/telegram`, `secret_token =
webhook_secret` and `allowed_updates = ["message", "callback_query",
"my_chat_member"]`. For the Mini App, set the Web App URL (BotFather
`/newapp` or the menu button set by `set_menu`) to the Deployment's HTTPS
origin.
