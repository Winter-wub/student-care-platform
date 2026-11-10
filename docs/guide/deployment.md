# Building a Deployment

How a Deployment is assembled from the Core, what `build()` refuses, how
Jobs run, and how sends, retries and webhook duplicates behave. A Module's
Notice to chosen Students (chat or email) is in [its own page](notify.md),
and a Module's settings edited from the back office in [theirs](settings.md).
Back to the [README](https://github.com/Winter-wub/student-care-platform#readme).

### Public API rules

- Everything a Deployment or Module author needs is re-exported from the
  crate root (`harpseal::Deployment`, `harpseal::Message`, ...). Free
  functions are grouped in `harpseal::render`, `harpseal::ui` and
  `harpseal::password`. Everything else is private.
- Public enums and config structs are `#[non_exhaustive]`; config structs
  come from TOML (`DeploymentConfig::from_toml`) or a `new` constructor.
- Every library crate has `#![warn(missing_docs)]` and `#![forbid(unsafe_code)]`.
- The Kernel's schema (`crates/harpseal/migrations/*.sql`) is not public API.

### Assembling a Deployment

```rust
let d = Deployment::builder()
    .config(DeploymentConfig::from_toml(TOML)?)
    .runtime(services)                       // from a runtime crate
    .channel(LineChannel::new(line, services.http.clone()))
    .channel(FakeChannel::new(secret))
    .verification(EmailOtp::new(config.otp.clone()))
    .module(MyModule)
    .build()?;                               // BuildError on any misconfiguration
d.migrate().await?;                          // Kernel first, then Modules; idempotent
let d = Arc::new(d);
let router = d.clone().router();             // serve with either runtime
d.tick(now).await?;                          // once a minute: claims and runs due Jobs
```

`build()` refuses: a missing config, runtime, Verification Method or Channel;
two Channels of one kind; two handlers for one Action id or Job kind; a Job
`schedule` outside the supported cron subset; a Permission declared twice; a
Role naming an undeclared Permission; a back-office route guarded by an
undeclared Permission; a Module that sends Mini App buttons when a Channel has
no Mini App URL; a Menu label key with no text in the default Locale; a
Mini App home link that is not a plain `/app/...` path the same Module
registered with `public`, or whose label key has no text.

#### A Module's Mini App page

A Deployment's own Module can serve a page inside the Mini App (LIFF on
LINE, Web App on Telegram) that shows the Student who opened it their own
data, and list it on the Mini App home:

```rust
impl Module for MyInfo {
    fn name(&self) -> &'static str { "my_info" }
    fn register(&self, r: &mut ModuleRegistrar) {
        r.messages(TEXTS)
            .public("/app/my-info", get(page))
            .mini_app_link("my_info.title", "/app/my-info", 50);
    }
}

async fn page(State(s): State<AppState>, me: AppStudent) -> Response {
    let body = html! { h1 { (me.display_name) } p { (me.student_code) } };
    me.page(&s, &me.t("my_info.title"), body)
}
```

- **`AppStudent`** is an axum extractor: the linked Student behind the App
  Session cookie (`id`, `student_code`, `display_name`, `locale`, the
  Channel kind, and `attributes`). It reads the cookie and the account's
  active Link on every request, for LIFF and Telegram Web App sessions
  alike (the partitioned cookie of Telegram Web included). Read data by
  `me.id` only: nothing in the path, query or body chooses the Student, so
  a changed URL can never show another Student's record.
- `attributes` holds only the `[roster.attributes.<key>]` marked
  `student_visible = true`. The default is `false`, so an attribute stays
  staff-only until the Deployment opts in, and a stored key that is no
  longer configured is never shown.
- Without a session (missing, forged or expired) the handler is never
  called: the Student gets the Kernel's entry page, which signs in through
  the platform and comes back to the same path. The same happens on every
  new launch (a request that does not come from one of the Deployment's
  own pages, see "Mini App" in [channels.md](channels.md)): the old
  session is dropped first, so a link from the chat to your page always
  shows the Student who opened it. An account whose Link was
  revoked gets the "go back to the chat" page. For an HTMX request both
  answer `HX-Redirect`, so the whole page reloads: to the same path for a
  `GET`, else to the page the request came from (`HX-Current-URL`, kept on
  `/app`), because HTMX follows the redirect with a `GET`. None of these
  is a 500.
- **Changing data is CSRF-checked by the extractors.** For any method but
  `GET`, `HEAD` and `OPTIONS`, both extractors refuse with 403 a request
  whose `Sec-Fetch-Site` is `cross-site` (another bot's Mini App inside
  Telegram Web shares the partitioned cookie). `AppStudent` then requires
  the session's token in the `X-CSRF-Token` header, which HTMX sends from
  a page rendered with `me.page`. A plain `<form method="post">` takes
  `AppForm<T>` instead: it also accepts the `_csrf` field
  (`input type="hidden" name="_csrf" value=(me.csrf_token())`), then
  decodes the body as `T` and gives `f.student` and `f.form`
  (urlencoded only, at most 64 KiB). A handler cannot skip the check.
- `AppForm` does not read `multipart/form-data`: a plain multipart
  `<form>` always gets 403 (it fails closed). Send a file with HTMX
  (`hx-encoding="multipart/form-data"`) or `fetch`, which carry the
  `X-CSRF-Token` header that `AppStudent` checks.
- `OPTIONS` counts as safe, like `GET` and `HEAD`: never change data on
  it, for example through `any(..)` routing.
- `me.t(key)` and `me.tf(key, args)` read texts in the Student's Locale
  with the back office's overrides. `me.page(&s, title, body)` renders the
  Mini App shell (branding, CSRF header for HTMX,
  `Referrer-Policy: same-origin`). Links inside your page are plain
  same-origin links: do not strip the `Referer` (`rel="noreferrer"`),
  or an older webview without Fetch Metadata counts each one as a new
  launch. Every `/app` response,
  whatever a handler returns (a fragment, JSON, a file), gets
  `Cache-Control: no-store` and the Mini App's `frame-ancestors` CSP.
- **Put the page under `/app/`.** Only there does it get the App Session
  cookie (`Path=/app`), those headers and the deep link back through the
  sign-in. `mini_app_link` refuses any other path, and `/app/` itself, at
  `build()`. A page registered elsewhere would always show the entry page.
- `mini_app_link(label_key, path, order)` adds a link to the home below
  the Student's card: every Module's links (the Kernel's too, should it
  add one) sorted by `order`, equal orders in registration order.
- Test it at the seam: `Browser::open_mini_app(&fake, external_id, path)`
  signs in on the Fake Channel. See `deployments/example` (`my_info.rs`)
  and `crates/harpseal-testing/tests/app_module*.rs` for LINE and Telegram.
- **A link to the page from outside the chat** (an email, a web page):
  `cx.mini_app_url(kind, path)` in a Job, `state.mini_app_url(kind, path)`
  in a page handler or `deployment.mini_app_url(kind, path)` give the URL
  a Mini App button to `path` opens on that Channel (the Channel's
  `MiniApp::launch_url`), `None` when the Channel takes no traffic or has
  no Mini App; `mini_app_urls(path)` gives `(ChannelKind, url)` for every
  Channel that has one. A chat message needs no URL: send
  `Button::mini_app(label, path)`. A LINE URL
  (`https://liff.line.me/<LIFF id>/app/fees`) opens the page in LINE from
  anywhere, so it works in an email; a Telegram Web App URL works only
  inside Telegram (in a browser the page asks the Student to open it from
  the chat), so an email should link to LINE. `path` is a page under
  `/app` (a debug build asserts it).
- **Only to Students who linked LINE.** A notify email goes to Students
  the chat does not reach: no live Link, or every live Link unreachable
  (they blocked the account). The LIFF URL opens the page only for a
  Student with a LINE Link; anyone else lands on "not linked" in a chat
  they never joined. So put the URL only in the email of Students with a
  live LINE Link, and tell the others how to add the Official Account and
  link first. The example's fee reminder does both, as two Notices under
  one key with the call's budget split between them
  (`reminders/notice.rs`), and treats a broken stored switch
  (`Error::SettingNotSet`) as "no link" rather than failing the send.

#### Jobs

A Module registers Job kinds (`r.job(JobSpec::new("news.send"), handler)`, or
`JobSpec::scheduled("news.digest", "0 7 * * 1-5")` for a recurring one) and
enqueues them with `d.enqueue(kind, payload, run_at)`, `enqueue_unique(key,
…)` (nothing inserted while a Job with that key exists), or
`jobs.insert(kind, &payload, run_at)` for a `Statement` to put in its own
`batch`; `PageState::jobs` and `JobContext::jobs` give pages and handlers the
same handle. An unregistered kind is `Error::Invalid`. Once a minute
`Deployment::tick`:

1. enqueues the scheduled Jobs due in this minute or the one before (cron
   subset: five fields, each a list of `*`, `*/n`, `a`, `a-b`, evaluated in
   `utc_offset`, day of week 0-7 with 0 and 7 = Sunday, the two day fields
   OR-ed when neither starts with `*`; key `cron:<kind>@<epoch minute>`, so
   several ticks in one minute run it once; a minute missed by two ticks in a
   row is skipped);
2. marks `failed` (`lock expired`) every `running` Job whose 15-minute lock
   passed with no retries left;
3. claims up to 25 due Jobs with one `UPDATE … RETURNING` (queued with
   `run_at <= now`, or running with an expired lock and retries left; a new
   lock token, `attempts + 1`), so concurrent ticks never share a Job;
4. runs each handler and writes the outcome only if the lock token still
   matches: `done`; `queued` again 1, 5 then 30 minutes later
   (`max_attempts` counts retries, default 3, so up to 4 runs); or `failed`
   with `last_error` (cut to 500 characters). A handler panic on native is a
   failed run of that Job only;
5. deletes `done` rows after `[retention] job_days` (30) and `failed` rows
   after 90 days (or `job_days`, if longer), at most 500 of each per tick.

On Workers (`panic = "abort"`) a handler panic aborts the whole tick: every
other Job that tick claimed has used an attempt and waits out its 15-minute
lock before it is re-claimed. A unique key (`enqueue_unique`, never starting
with `cron:`, which the scheduler reserves) is held until its row is pruned,
30 days after `done` or 90 after `failed`: it dedupes, it does not lock, so
a resend or a deliberate retry needs a fresh key (for announcements, not a
stable per-announcement key).

Handlers must be idempotent: a Cron Trigger can fire twice and an expired lock
re-runs a Job. `JobContext::progress(note)` saves a note while the run holds
its lock. `GET /admin/jobs.json` (Permission `kernel.audit.view`) returns the
counts per state and the 50 newest running and failed Jobs, without payloads.

#### Sends, retries and webhook duplicates

- **Metering.** Each `SendReceipt` carries `metered_units`: one per Push
  request to one recipient (a long text split over requests costs one per
  request; Telegram 0), charged to the Message that opens the request.
  `usage_daily` adds them to `metered`, and Messages a Channel fails on to
  `failed` (a blocked-account skip counts nowhere). Every `outbound_message`
  row keeps its batch key, its Message's index in the batch (`msg_index`)
  and the units counted for it (`units`, migration 0010), so a resend of
  the same batch meters each Push request exactly once (below).
- **Quotas (#26).** `/admin/usage` charts each Channel's month against
  `[quotas.<kind>]`; from 80 % every Staff page shows a banner (red at 100
  %) and the tick's `kernel.usage.watch` (every 10 minutes, only with a
  Quota in force) emails active Administrators once per month per level.
  For LINE, an Administrator picks the Official Account package in the back
  office and the limit follows it, `[quotas.line]` being the default
  (#115, [quotas.md](quotas.md)).
- **Partial sends.** When a Channel stops part-way, what went out is
  recorded (`outbound_message` rows and usage; a Message cut off after some
  of its parts gets a row with `error = 'partly sent: …'`) and
  `Deployment::send_batch` returns `Error::PartialSend { sent, total,
  source, resume, key, .. }` (`#[non_exhaustive]`: match it with `..`,
  build it with `Error::partial_send`). `Error::root()` gives the cause
  (`RateLimited`, `Unreachable`). A resume in the same process passes the
  whole batch and `SendOptions::resume_after(&err)` (ADR-0010), which also
  restores the batch key (the fresh one an unkeyed `send_batch` chose):

  ```rust
  let opts = SendOptions::new().key(format!("{job}/{account}"));
  match d.send_batch_with(&to, None, &msgs, &locale, &opts).await {
      Err(e @ Error::PartialSend { .. }) => {
          // in the same run, e.g. after waiting out a `RateLimited`:
          d.send_batch_with(&to, None, &msgs, &locale, &opts.resume_after(&e)).await?;
      }
      other => { other?; }
  }
  ```

  The Kernel sends Messages `sent..` and hands the Channel the adapter's
  `resume` token (`Channel::send_batch_resumed`). Nothing before Message
  `sent` goes out again. On LINE the token rebuilds the request that
  failed, and those after it, with the same objects and `X-Line-Retry-Key`
  even when the stopped Message shared its first request with earlier
  Messages: LINE answers 409 to a request it already took and no part
  reaches the Student twice. Telegram and the Fake Channel give no token
  (they send one part per request and have no retry keys): the resume
  starts at the first part of Message `sent`, whose parts already delivered
  go again. That is the accepted trade-off there: a repeated Card beats a
  lost one. The token is not stored: a later run (the Job's next attempt)
  resends the whole batch with the same key instead
  (`send_batch_with_key`), which is metered exactly too and on LINE
  delivers nothing twice either (the same requests, the same keys). An
  unkeyed send cannot be retried exactly in a later run: its key lives
  only in the error. LINE ignores a token written for another batch key,
  another account or other Messages (without a log: the adapter has no
  logging port) and then resumes from the first part of Message `sent`,
  delivering those parts again rather than losing them.
- **Idempotency keys.** `Deployment::send_batch_with_key(to, reply, msgs,
  locale, key)` (or `send_batch_with` and `SendOptions::key`) passes a
  stable key (e.g. `<job id>/<account>`); LINE turns it
  into one `X-Line-Retry-Key` per Push or multicast request (a UUID derived
  from the key, the recipients, the objects and how often that same request
  already occurred in the batch, so identical requests in one batch never
  share a key), so a retry of a request
  that already went out is answered 409 by LINE and counted as sent. Without
  a key every batch gets a fresh one. `Multicast::send_many` takes the key
  too and reports whole groups of 500 on failure.
- **Webhook duplicates.** `Inbound` carries `event_id` (LINE
  `webhookEventId`, Telegram `update_id`) and `redelivery` (LINE
  `deliveryContext.isRedelivery`). The Kernel claims `(channel, event_id)` in
  `inbound_event` with one conditional insert before handling an event and
  skips an id it has seen (still 200; counted in `WebhookReport::duplicates`),
  so a redelivered LINE event can no longer fall back to a second, metered
  Push. `redelivery` is informational. A claim gets `done_at` once its event
  was answered or logged. A claim still unfinished 60 s after it was taken
  (the handler was killed) is logged once as an `inbound.lost` Security
  Event (Channel, sender, event kind and id; never the payload) and never
  answered: by the next redelivery of the event, else by the tick (at most
  100 per tick, `TickReport::events_lost`). A redelivery within the 60 s is
  answered 200 as a duplicate, so the platform stops retrying; the tick is
  what catches it then (ADR-0009). `Deployment::tick` deletes claims older
  than 7 days, at most 500 per tick.
- **Exact metering on a resend (#62).** Before a keyed send, the Kernel
  reads the rows earlier attempts with that key wrote for the account. Per
  Message: every unit the Channel sent now counts (the platform charged
  it); a unit the platform answered as already accepted (LINE 409,
  `SendReceipt::replayed_units`) counts only beyond the `units` its row
  records, because it went out on an earlier attempt whose answer or
  whose write was lost. So a recorded Message replayed whole adds 0, a
  resume after a partial send adds only its new Pushes, and a resume after
  a write that failed counts the replayed Pushes once. The Message's row
  is updated (units, platform id; "partly sent" cleared once it is
  complete), never written twice. A row of another kind at the same index
  is left alone (the key was reused for other Messages). Rows from before
  migration 0010 have no index and are never matched. Two sends with the
  same key running at once are not supported: each reads before it writes.
- **Building `Inbound` events.** Each variant is `#[non_exhaustive]`: build
  events with `Inbound::text`, `Inbound::action`, `Inbound::file`,
  `Inbound::follow`, `Inbound::blocked` and `Inbound::unblocked`, plus
  `with_event_id` and `with_redelivery`, and match them with `..`.
- **Transport errors.** `ReqwestHttp` strips the request URL from every
  reqwest error (`without_url`), and `WorkerHttp` scrubs URLs and
  `bot<digits>:<secret>` tokens from the JS errors of `fetch` (an "Invalid
  URL" error quotes the URL), so bot tokens
  never reach logs or `security_event`; the Telegram adapter still redacts
  its token too. D1 writes without `meta.changes` fail with `Error::Db`
  instead of reporting 0 rows.

Storage follows ADR-0007: `Services::new_id()` makes UUIDv7 text ids from the
Clock and Random ports, `Services::now_ms()` gives integer milliseconds UTC,
and `audit_log` (plus `security_event` and `consent` deletes) are refused by
triggers.

## Writing from code: the same audited paths as the back office

A CLI, a seed or a customer Module can make the back office's writes
without HTTP. Each function takes the acting `Actor` and a `RequestCtx`
(`RequestCtx::cli()` for a command line), checks that the actor is an
active Staff member holding the Permission the page requires, and audits
the write as that Staff member. They wrap the page handlers' services and
add no logic of their own:

| Function | Permission | Audit |
|---|---|---|
| `harpseal::roster::upload(d, name, bytes, mode, actor, ctx)` → `Uploaded` | `kernel.roster.import` | `roster.import.upload` |
| `harpseal::roster::confirm(d, id, actor, ctx)` → `Confirmed` | `kernel.roster.import` | `roster.import` |
| `harpseal::modules::announcements::create_draft(d, &NewAnnouncement, actor, ctx)` | `announcements.send` | `announcement.create` |
| `harpseal::modules::announcements::confirm(d, id, version, actor, ctx)` | `announcements.send` | `announcement.send` / `.schedule` |
| `harpseal::texts::set_override(d, key, locale, value, actor, ctx)` | `kernel.texts.edit` (`harpseal::texts::TEXTS_PERMISSION`; `kernel.setting.edit` before #165) | `setting.update` |
| `harpseal::branding::set_menu_images(d, locked, unlocked, actor, ctx)` / `reset_menu_images(d, actor, ctx)` → `MenuImagesSaved` | `kernel.setting.edit` | `branding.menu_images` / `.reset` |
| `harpseal::branding::set_name(d, name, actor, ctx)` / `set_primary_color(d, color, actor, ctx)` → changed? | `kernel.setting.edit` | `branding.update` (`.reset` once nothing is saved) |
| `harpseal::branding::set_name_as_system(d, name, ctx)` / `set_primary_color_as_system(d, color, ctx)` | none: seed and CLI code | `branding.update` (actor `system`) |
| `harpseal::usage::set_line_package(d, LinePackage, actor, ctx)` → changed? | `kernel.setting.edit` | `setting.update` / `setting.reset` |
| `harpseal::usage::set_line_package_as_system(d, LinePackage, ctx)` | none: seed and CLI code | `setting.update` / `setting.reset` (actor `system`) |
| `d.settings().save(key, value, actor, ctx)` ([settings](settings.md#saving-from-code)) | the Setting's own | `setting.update` |
| `d.save_setting_as_system(key, value, ctx)` | none: seed and CLI code that holds the `Deployment` | `setting.update` (actor `system`) |

**Announcement files** (#164). `NewAnnouncement::files` takes images and
PDFs: `AnnouncementFile::image(n, name, content_type, bytes)` (slots 1 to
4: PNG or JPEG told by the bytes and named as what it is, at most 1 MB and
4096 pixels a side) and `AnnouncementFile::pdf(n, name, content_type,
bytes)` (slots 1 to 3: named `.pdf`, at most 10 MB, no active content).
These are the create form's checks; `content_type` must also agree with
the bytes, and a slot out of range is refused (`…bad_slot`). A refusal is
`Error::Invalid` naming every catalog key (the text checks' and the files'
together), and nothing is written. `create_draft(d, &new, …)` copies the
files' bytes once; `create_draft_owned(d, new, …)` moves them. Natively one
call may fill all three PDF slots; **on Workers attach at most one PDF per
call**, as the form does (one new PDF per save): a 10 MB PDF held twice or
three times counts against the isolate's 128 MB. There is no alt-text field, as on the form: the body
places image `N` with its alt (`![alt](image:N)`), and an image it does
not place is shown after it with the alt `<label> N`. The audit row lists
slots, never names or bytes.

**Branding and the LINE package** (#164) take the page's checks: a name of
1 to 80 characters with no control characters, a `#rrggbb` colour that
white text reaches a 4.5 contrast on (`Error::Invalid` naming
`kernel.branding.err_contrast` and the ratio otherwise), a Custom package
of 1 to 100,000,000 and a Deployment with LINE. Each setter keeps the other
values (the logo too); a value equal to `deployment.toml`'s is no setting
and follows the file; the same value again writes nothing (`Ok(false)`); a
save in between is `Error::Conflict`. The Staff variants accept only an
active Staff member holding `kernel.setting.edit` (a Student, a Job or
`Actor::System` is `Forbidden`); the `*_as_system` ones are for a seed or
the CLI, never for a Staff action. Read what is saved with
`branding::saved(d)` (`None` while `deployment.toml` applies) and
`usage::line_package(d)` (`LinePackage::FollowConfig` while nothing is
saved), so a seed can leave an Administrator's choice alone.

**Roster attribute values** (#164). `harpseal::roster::attribute_values(d,
key, &ValuesScope)` lists the distinct values of `[roster.attributes.<key>]`
with how many Students hold each, for a Module's filter or report, so it
never reads `student.attributes` itself. A group of one names a Student
(the GPA of the only Student in a programme and year is theirs), so the
attribute counted and every scope attribute must be an `enum` or an
`integer` marked `student_visible = true`; a `number`, `text` or `date`
attribute, or a Staff-only one, is `Error::Invalid`. `ValuesScope::active()` counts
`active` Students; set `include_left` to count `left` ones too, and
`.with(key, &[values])` to keep Students whose other attribute is one of
those values (as an Audience's attribute filter, at most 50 values).
Values are the stored JSON value as text (`2`, `MSC`), numeric attributes
in numeric order; a Student without the attribute is not counted; an
undeclared key is `Error::Invalid`. It checks no Permission and writes no
audit row, and a count of 1 can still single someone out: a page that
shows the counts to Staff checks `kernel.student.view` first, and one that
shows them to Students hides small groups.

`RequestCtx::cli().seeded()` adds `"seed": true` to the `after` of every
audit row written with it, so demo data stays distinguishable
(`harpseal seed-example` uses it). A confirmed roster import is applied by
the Job `kernel.roster.import` on the next ticks.

### Calling Core services from a Module page

A Module's back-office handler reaches the functions above through
`PageState::deployment()` (and a Job handler through
`JobContext::deployment()`). Act as the signed-in Staff member with
`Actor::staff(&staff)` and pass the request's `RequestCtx` (extract it like
`CurrentStaff`): the service checks that Staff member's Permission and
audits the write with the request's address, user agent and request id, as
the Core page does. The route's own Permission is not enough: a Staff member
who may open `/admin/reminders` but lacks `announcements.send` is refused.
`ui::service_error` answers that refusal with the same 403 page the Core
page shows, a `Conflict` (the row changed meanwhile) with a 409 page asking
to reload, and anything else with a 500 page that hides the error text.

```rust
use harpseal::web::axum;
use axum::Form;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Redirect, Response};
use harpseal::audience::{Audience, MAX_STUDENTS, parse_student_ids};
use harpseal::modules::announcements::{NewAnnouncement, create_draft};
use harpseal::{Actor, AppState, CurrentStaff, RequestCtx, ui};

async fn announce(
    State(s): State<AppState>,
    staff: CurrentStaff,
    ctx: RequestCtx,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let ids = form
        .iter()
        .find(|(name, _)| name == "ids")
        .map(|(_, v)| parse_student_ids(v))
        .unwrap_or_default();
    // No list is no filter (everyone); over MAX_STUDENTS the Core refuses.
    if ids.is_empty() || ids.len() > MAX_STUDENTS {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    }
    let mut audience = Audience::everyone();
    audience.students = ids;
    let mut new = NewAnnouncement::new("เตือนชำระค่าธรรมเนียม", "กรุณาชำระภายในวันศุกร์นี้");
    new.audience = audience;
    match create_draft(s.deployment(), &new, &Actor::staff(&staff), &ctx).await {
        Ok(id) => Redirect::to(&format!("/admin/announcements/{id}")).into_response(),
        Err(e) => ui::service_error(&s, &staff, &e),
    }
}
```

The draft opens on the Core's detail page, where Staff check the Audience
and send it (`confirm` sends from code). Use the borrowed `Deployment`
within the request or run only: a clone kept in a Module field, a `static`
or a handler forms a reference cycle (the Deployment owns the handlers)
and is never freed. A Job has no signed-in Staff member; one acting for
Staff passes `Actor::Staff` with the id its page saved in the payload, and
that Staff member's Permission is checked again when the Job runs. The
example's `deployments/example/deployment/src/reminders.rs` has the
whole page.

### axum and maud, the flash and logged errors (#162)

A Module's pages take axum from the Core: `use harpseal::web::axum;`, then
`use axum::extract::State;` and so on as usual. The Deployment then has no
`axum` dependency of its own, so its `MethodRouter`, extractors and
responses are always the Core's types. Only the features the Core turns
on are guaranteed, native and on Workers: `form`, `query` and `multipart`
(`Form`, `Query`, `Multipart`); `axum::http` is the `http` crate. A page
that needs another (`Json`, `MatchedPath`, …) adds
`axum = { version = "0.8", default-features = false, features = ["json"] }`
to the Deployment; Cargo merges it into the Core's axum, so the types
stay the same. axum 0.8 and maud 0.27 are part of the Core's public API:
a Core release that moves either to another minor version is breaking. `maud` is the one web crate a Deployment still names: `html!`
expands to `extern crate maud`, which only a direct dependency satisfies.
Keep it at the Core's requirement, `maud = "0.27"`, so Cargo resolves one
`maud` for both and `Markup` is the type `ui::back_office` takes
(`harpseal::web::maud` is the same crate, for paths). The example's
`deployments/example/deployment/Cargo.toml` depends on `harpseal` and
`maud` only.

After a POST, answer `ui::redirect_with_flash(&s, &headers, to, ok, key)`
instead of redirecting with `?done=<code>`: a `303` to `to` (an HTMX
request gets `200` with `HX-Redirect`) with the Core's one-shot flash
cookie. `to` must be a path under `/admin`; anything else goes to
`/admin`. The next back-office page draws the text of catalog key
`key` at the top of the main column, in the Staff member's Locale (green
when `ok`, red otherwise), and a reload no longer shows it. `key` is one
of the Module's own catalog keys (letters, digits, `.` and `_`; any
other key sets no flash, and a key the catalog lacks shows as itself).

```rust
async fn cancel(
    State(s): State<AppState>,
    staff: CurrentStaff,
    headers: HeaderMap,
    uploads: Uploads,
    ctx: RequestCtx,
    Path(id): Path<String>,
) -> Response {
    let to = "/admin/fees";
    match uploads.discard(&id, &ctx).await {
        Ok(true) => ui::redirect_with_flash(&s, &headers, to, true, "fees.cancelled"),
        Ok(false) => ui::redirect_with_flash(&s, &headers, to, false, "fees.not_cancelled"),
        Err(e) => ui::service_error(&s, &staff, &e),
    }
}
```

A 500 from `ui::service_error` carries an `ErrorReport`, so the runtime
logs it as it logs the Core's errors: one line with the Module's name, the
error's kind and the route as the Module registered it, then the request
id (`error: fees: db at /admin/fees/{id}/cancel (request 9f2c…)`; stderr
natively, `console.error` in `wrangler tail` on Workers). The error's
message, which may hold a Student's data, is never logged or shown. Answer
a failure with `service_error` rather than a bare
`StatusCode::INTERNAL_SERVER_ERROR`, which nobody sees in the log. A page
without the back-office shell (a Mini App page) attaches the report
itself, with a fixed area name:

```rust
use harpseal::{Error, ErrorReport};

fn failed(e: &Error) -> Response {
    let page = StatusCode::INTERNAL_SERVER_ERROR.into_response();
    ErrorReport::attach(page, "fees.student", e)  // logs "error: fees.student: db (request …)"
}
```

## Reading spreadsheet uploads in a Module

`harpseal::sheet` is the reader the roster import uses, public so a
Module parses its own uploads (fees, grades) with the same rules:
`Workbook::open(bytes, file_name)` reads a `.csv` (one unnamed sheet) or
every worksheet of an `.xlsx` (at most 50; `Sheet::is_hidden` marks hidden
ones, skip and list them). `sheet.table(&TableSpec::new([...]))` finds the
header row among the first 10 non-empty rows (title rows above are
skipped; a required column repeated in the header is an error) and yields
`Row`s with their sheet name and 1-based row number. By default every row
under the header is data. `.skip_footer(["Amount"])` skips totals and notes
rows at the bottom whose key cell holds a label (text with letters that is
not a key) and that hold nothing else except under the numeric columns you
name; a row with only numbers and no label is data (an id was lost). Show
`Table::footer()` (sheet, row, text) to Staff, never drop it silently; if
the rule would skip every row under the header, `table()` returns
`TableError::OnlyFooter`. The rule cannot tell `Pla Thong,,500` (a name in
the id column) from `Total,,500`: keyed by names or letter codes, pass
`.is_key(|s| ...)` (a closure, e.g. over the codes you know) before turning
the footer on. For money, check the footer against the data:
`table.check_footer_total("Amount", 2)` fails (`TotalError::Mismatch`)
when a totals row is not the exact sum of the rows above it, which is how
a data row read as a footer shows itself. `footer()` and `into_parts()`
are `#[must_use]`. An Excel error cell is `Cell::Unreadable` with its code
and cell (`#REF!`, `C10`), never text.

Numbers: `sheet::number_scaled(text, 2, Blank::Zero)` reads money exactly
in satang (`12,500.50` is `1_250_050`; more decimals than the scale, a
value beyond `±i64::MAX` or a scale above 18 is refused, never rounded).
Numeric XLSX cells, calculated ones included, arrive as text: a
fractional part rounded to 15 significant digits, as Excel shows it
(`12500.000000000002` is `12500`), a whole number exact below 2^53 (a
16-digit reference stays itself); 2^53 or more is `Cell::Unreadable` with
code `sheet::TOO_LONG`. `sheet::number` gives an `f64` of at most 15
significant digits. Both accept `12,500`, ` 12500 `,
`12,500.00`, `+5`, `.5`; `.` is always the decimal point. They refuse
`(500)`, `฿`, Thai digits, spaces or no-break spaces inside the number, the
minus sign U+2212 and exponents: normalise those first. The rustdoc of
`harpseal::sheet` has a worked example.

## A Module's spreadsheet upload page

A back-office page that takes a file uses `ModuleRegistrar::sheet_upload(
permission, path, route)` instead of `back_office` for its upload route
(Core change in #74). The route is guarded like any back-office route
(only Staff holding `permission`; the Permission is checked before the
body is read), and the CSRF token must be in the `_csrf` part (put
`staff.csrf_field()` before the file input) or the `X-CSRF-Token` header.
The body may be as large as the roster import's: a file of
`harpseal::MAX_UPLOAD_BYTES` (4 MiB) plus 64 KiB of other fields; above
that the answer is 413. Other back-office forms stay at 64 KiB.

The handler extracts `SheetUpload` (or `Result<SheetUpload,
UploadRejection>` to answer refusals with its own page): the `file` part,
its cleaned `file_name`, its `kind` (`SheetKind::Csv` or `Xlsx`, told by
the bytes — a ZIP signature or UTF-8 text — and the name must agree, else
415), and the form's other text parts in `fields`. An empty or missing
file is 400, a file over 4 MiB 413, and a `.csv` that is not UTF-8 (a
Thai CSV saved as TIS-620 / Windows-874) 415 with "save as CSV UTF-8".

Preview, then confirm, like the roster. Back-office handlers extract
`Uploads`: the kept uploads of the Module that registered the route, as
the signed-in Staff member sees them. By default only the uploader sees an
upload (another Staff member with the same Permission gets `None`, as if
the id did not exist); `uploads.shared_with("<module>.<permission>")`
opens every upload of the Module to Staff holding a Permission you choose.

1. Read it: `up.workbook()` is `harpseal::sheet` with the roster's limits.
2. Keep it: `uploads.keep(up, &ctx)` records it (Kernel table
   `module_upload`, migration 0014: uploader, file name, kind, size,
   SHA-256, an expiry 24 hours later, `UPLOAD_KEEP_MS`), audited
   `module.upload`, then stores the file under
   `module-upload/<module>/<id>.<kind>`. Redirect to a preview page
   carrying the returned id.
3. Preview: `uploads.open(&id)` gives the file back (`None` when it is not
   visible, was discarded or expired). Show the rows, the problems with
   sheet and row, and the skipped footer rows and hidden sheets.
4. Confirm: `open` again (it refuses, `Error::Invalid`, a file whose hash
   changed since the upload), then `uploads.confirm(&id, &ctx)`, which
   pins it for the Job (audited `module.upload.confirm`; it is kept
   `CONFIRMED_KEEP_MS`, a week, from now, so a confirm a minute before the
   24 hours run out still reaches the Job), then enqueue your import Job
   with the id in its payload (`enqueue_unique` with a key on the id
   stops a double click).
5. The Job reads it with `KeptUpload::open_confirmed(&services, "<module
   name>", &id)`, imports, then calls `KeptUpload::finish`. When it finds
   no upload, it is done only if its own rows show the import happened
   (the example stores the upload id beside each row); otherwise return an
   error, never `Ok`.
6. Cancel: `uploads.discard(&id, &ctx)`, audited `module.upload.cancel`.

The tick deletes every upload past its expiry (50 per tick,
`TickReport::module_uploads_swept`), so a preview nobody confirmed leaves
no file behind. Backups leave kept uploads out, rows and files (see
[backups](backups.md)).

`deployments/example/deployment/src/fees.rs` is a worked example: outstanding
fees, one term per sheet, imported into the Module's own table by the Job
`fees.import`.

### Help beside the file input, and the next step (#127)

Declare the file's columns once, with the same path as the upload route.
It is optional: a Module that declares none keeps building and its panel
shows the file rules only.

```rust
pub const COLUMNS: &[UploadColumn] = &[
    UploadColumn::required("รหัสนักศึกษา", "grades.col.code", "68010001"),
    UploadColumn::required("รหัสวิชา", "grades.col.course", "SB101"),
    UploadColumn::optional("หมายเหตุ", "grades.col.note", ""),
];

r.sheet_upload("grades.import", "/grades/upload", post(upload))
    .sheet_columns("/grades/upload", COLUMNS);
```

Each column is its header as it must appear in the file, the catalog key
of what it holds in Staff's words, whether every row must fill it, and a
fictional example value (empty for none). With columns, the Core serves
`GET /admin/grades/upload/template.csv` (the header row, UTF-8 with a BOM)
to Staff holding the upload's Permission. The path has no parameters.
`build()` refuses columns for a path with no `sheet_upload` in the same
Module (also one registered with `back_office` instead) or with a
parameter (`InvalidRoute`), a template path another route already takes
(`DuplicateRoute`) and an unknown label key (`UnknownText`).

Put the panel beside the form, the roster's layout:

```rust
div class="cols" {
    form class="card" method="post" action="/admin/grades/upload" enctype="multipart/form-data" { … }
    (harpseal::ui::upload_help(&s, &staff, "/grades/upload"))
}
```

`ui::upload_help` draws a card with the file rules (CSV or XLSX, at most
4 MB, CSV saved as UTF-8), each declared column (header, label, "ต้องมีค่า"
or "เว้นว่างได้", the example) and the template link, shown only to Staff
holding the upload's Permission. `.cols` puts it
beside the form from an 860 px content width and under it on a phone.

Offer the next step on the result page **once the import Job has
written its rows**, not right after the confirm: until then say the
import is queued (or that it failed). Read that result from the Module's
own tables, keyed by the upload id: the Job's `KeptUpload::finish`
deletes the kept upload, so `uploads.open(&id)` finds nothing on a reload
after the next tick. Keep the uploader rule `Uploads::open` applied: store
`kept.staff_id` with your record and show the result only to that Staff
member (the example also puts the uploader in the Job's payload, so a Job
that failed after its upload was gone answers only them). The example
keeps one `fees_import` row per upload (written in the Job's batch, with
`staff_id`) and the upload id on each balance (indexed, migration
`0003`):

```rust
// GET /admin/grades/{id}
if let Some(done) = imported(&s, &id, &staff.id).await? {  // your table: upload id and uploader
    let back = s.catalog.t(&staff.locale, "grades.back_to_list");
    return page(html! {
        div class="flash ok" { … }
        (harpseal::ui::next_step(&s, &staff, &done.student_codes, (&back, "/admin/grades")))
    });
}
// Not imported yet: the kept upload's preview, "queued" or "failed".
```

`next_step` offers "แจ้งนักศึกษา" to Staff holding `announcements.send`
(when the Announcements Module is mounted): a new announcement whose
Audience is exactly those student codes (`ui::announce_href`; repeats
are dropped). A link is a URL, so it names Students only up to
`ui::MAX_PREFILL_QUERY` (7 KB) of query and `ui::MAX_PREFILL` (a
thousand) codes; the 7 KB binds first for usual codes (about 790
eight-digit codes). Above either, or with an empty list, Staff choose the Audience
themselves on the form, which takes any list they paste. Nothing is saved
or sent by the link: the draft goes through the usual review. The way
back is always there. Pass an empty list when the file cannot name its
Students exactly (the example does when a later file replaced some of its
balances).

For a Module's list with nothing yet, use `ui::empty_state(text,
Some((label, href)))`: one sentence on what goes there and the action that
fills it, passed only when the Staff member holds its Permission. A search
or filter with no match uses `ui::no_match(text, (label, href))`, whose
link clears it. A posting form reads busy until the next page (#131); give
a form whose submit answers a download or `204 No Content` the attribute
`data-no-busy`, or it reads busy for 30 s.

### A field of student codes: the Student picker (#146)

A form that takes student codes uses `ui::StudentPicker` instead of a bare
textarea. Staff search by the start of a code or part of a name, add hits
as chips, paste many codes at once, and see unknown codes and left
Students flagged, with the count against your limit. Name its route once
as a `ui::PickerRoute` constant, mount it under your own Permission, then
render the picker inside your `<form>` with the same constant (so the two
cannot point at different paths):

```rust
pub const PICK: ui::PickerRoute = ui::PickerRoute::new("/reminders/pick");

// In `register`:
r.student_picker(PERMISSION.id, PICK);

// In the page handler, inside the form:
let field = ui::StudentPicker::new("ids", PICK, &t("reminders.ids"))
    .max(harpseal::MAX_NOTIFY)
    .codes(ids) // already picked, e.g. from `?ids=`
    .render(&s, &staff)
    .await?;
```

- **What your handler gets:** the same field as a textarea would post,
  `ids` here, codes separated by newlines. Read it with
  `audience::parse_student_ids` and check the limit and the codes
  yourself: the picker shows problems, it does not refuse a post.
- **Without JS** (or when HTMX does not load) the field is the textarea,
  with the same `id`, `name` and content. A small inline script turns the
  picker on and disables the textarea, so the field is posted once.
- **Who sees what:** the route answers only Staff holding the Permission
  you named (a POST also needs the CSRF token HTMX sends). Names are
  searched and shown only to Staff who also hold `kernel.student.view`;
  others search by code and see codes, and the page never reads their
  names. Search lists at most 8 Students: codes that start with what was
  typed, or with it in upper case ("b65" finds "B6512345"), read
  through the code's index, then, from two
  characters on, names that contain it.
- **Texts are neutral** ("เลือกได้ไม่เกิน 40 คน เกินมา 2 คน", "2 คนพ้นสภาพ
  แล้ว"), so any form can use the picker (Saved Audiences, bulk actions).
  Say what a left Student means for your action in your own help line.
- **Screen readers** hear one status line per search ("พบ 3 คน") and per
  change ("เพิ่ม 68010002 แล้ว เลือกแล้ว 2 คน"), not the whole list. Fast
  clicks queue: each change is sent after the one before has come back,
  carrying the latest list.
- **Field names** are lowercase ASCII letters, digits and `_` (at most
  32), since they become element ids, and not one of the picker's own
  (`q`, `pf`, `pmax`, `op`, `code`, `paste`); another name is
  `Error::Invalid`.
- **Starting points:** prefill with `.codes(…)` from your own data. The
  example's reminders page has "เริ่มจากรายการค้างชำระ" (`?from=fees`),
  which preselects the active Students who owe a fee, the first 40 by code.

## Permissions in Thai

A Permission is a short Thai sentence for Staff (#125): a verb and its
object, no id, no English. Add it with `thai`, which is `const`:

```rust
pub const IMPORT: Permission = Permission::new("grades.import", "Upload course grades")
    .thai("นำเข้าผลการเรียน");
```

The Roles page shows `Permission::label()`: the Thai sentence, else the
English `description` (a Module written before #125 keeps building and
shows that), else the id made readable. The id and the English
description stay in the technical details. Group headings use the title
of the Module's first back-office page, so a Module with a page needs
nothing more.

Label the Module's audit actions the same way, with a
`<module>.audit.<action>` text: an action `grades.import` reads the text
`grades.audit.import` ("นำเข้าผลการเรียน") in the audit log.

## Tag names Staff read (#157)

A Tag is stored by the name the roster or Staff gave it, often a code
such as `fee_due`. Name it for Staff with the text `tag.<name>`: in
`[texts]`, no Rust needed, or in a Module's defaults when the Module
chooses the Tag itself (the example's reminders register
`tag.fee_due` = "ค้างชำระค่าธรรมเนียม").

```toml
[texts.th]
"tag.scholar" = "ทุนการศึกษา"
```

Pages that show a Tag read `Catalog::tag_name(locale, name)`: the text
in the Staff member's Locale or the default Locale, else the stored name.
The stored name never changes, so a roster file keeps using the code.
Like Role and Locale labels, these texts are not on the Texts page (a
saved edit would not show there): change them in `[texts]`. A key with
no name after `tag.` is refused at build (`UnknownText`).

## A Module's back-office pages: navigation group and home card

Every back-office page shares the Core shell (#123): a skip link to the
content, the institution's bar, and the navigation in two groups,
**งานประจำ** (daily work) and **ผู้ดูแลระบบ** (administration), with the
current page marked `aria-current="page"`. Below 640 px the navigation folds
behind a "เมนู" button (a `<details>`, no JS); from 640 px it is a side
column. A group shows only the pages the Staff member's Permissions allow,
and disappears when it has none.

`back_office_page` lists a page under งานประจำ, which suits most Module
pages (fees, grades, requests). An administration page picks the other
group:

```rust
r.back_office_page("grades.import", "/grades", "grades.title", 60, get(page))
    .back_office_page_in(NavGroup::Admin, "grades.setup", "/grades/setup",
        "grades.setup_title", 95, get(setup));
```

Within a group, pages sort by their order (the Kernel uses 10 to 90).

### A status card on the home

`/admin` is the term status page: one ledger of status cards, each a
state, one fact and one next action. The Kernel shows the last roster
import, announcements, the message quota and the last backup (orders 10
to 40). A Module adds its own:

```rust
r.status_card("fees.import", "fees.home.title", 50, fees_card);

async fn fees_card(s: AppState, staff: CurrentStaff) -> harpseal::Result<StatusCard> {
    let owing = count_owing(&s).await?; // one query
    let tf = |k: &str, args: &[(&str, &str)]| s.catalog.tf(&staff.locale, k, args);
    Ok(if owing == 0 {
        StatusCard::new(CardState::Ok, tf("fees.home.clear", &[]))
    } else {
        let n = harpseal::ui::number(owing); // "1,250"
        StatusCard::new(CardState::Warn, tf("fees.home.owing", &[("n", &n)]))
            .action(tf("fees.home.open", &[]), "/admin/fees")
    })
}
```

- **Title.** `title_key` is a catalog key: the Core shows its text, also
  when the source fails, so Staff always know which card it is.
  `Catalog::tf` fills `{name}` placeholders in your texts.
- **States.** `CardState::Empty` (nothing yet: the fact says what to do
  first, such as "ยังไม่ได้นำเข้ายอดค้างชำระ", and the action starts it),
  `Ok`, and `Warn` (needs a Staff member's attention). The Core draws them
  in the fixed status colours, never the brand.
- **One fact, one action.** Write the fact as a short Thai sentence with
  the number that matters; link the action to a page the Staff member may
  open (check `staff.can(..)` when the best action needs another
  Permission, as the example's card does for "เลือกนักศึกษาที่จะเตือน").
- **Cheap.** The source runs only on the home, only for Staff holding the
  card's Permission, once per view. Keep it to one or two queries (D1
  bills per query and per row read): count with SQL, do not load rows.
- **Contained.** A source that returns `Err` shows "อ่านข้อมูลส่วนนี้ไม่ได้"
  under the card's title; the error text is never shown and the rest of
  the home renders.
- `build()` refuses a card whose Permission no Module declared
  (`BuildError::UnguardedStatusCard`) and a `title_key` with no text in
  the default Locale (`BuildError::UnknownText`).
- **Sources** are `async fn(AppState, CurrentStaff) -> harpseal::Result<StatusCard>`
  or an equivalent closure; `StatusSource` is sealed. Tests read a card
  back with `state()`, `fact()` and `action_link()`.
- **One after another.** The home runs the visible cards in order, not
  concurrently (no `futures` crate in the Core); the quota card reuses the
  usage read the banner already made for the request.

`deployments/example/deployment/src/fees/status.rs` is the worked example.
