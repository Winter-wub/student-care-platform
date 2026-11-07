# Building a Deployment

How a Deployment is assembled from the Core, what `build()` refuses, how
Jobs run, and how sends, retries and webhook duplicates behave. Back to the
[README](https://github.com/Winter-wub/student-care-platform#readme).

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
  the platform and comes back to the same path. An account whose Link was
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
  Mini App shell (branding, CSRF header for HTMX). Every `/app` response,
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
  %) and the tick's `kernel.usage.watch` (every 10 minutes, only with
  `[quotas]`) emails active Administrators once per month per level.
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
| `harpseal::texts::set_override(d, key, locale, value, actor, ctx)` | `kernel.setting.edit` | `setting.update` |

`RequestCtx::cli().seeded()` adds `"seed": true` to the `after` of every
audit row written with it, so demo data stays distinguishable
(`harpseal seed-example` uses it). A confirmed roster import is applied by
the Job `kernel.roster.import` on the next ticks.

## Reading spreadsheet uploads in a Module

`harpseal::sheet` is the reader the roster import uses, public so a
Module parses its own uploads (fees, grades) with the same rules:
`Workbook::open(bytes, file_name)` reads a `.csv` (one unnamed sheet) or
every worksheet of an `.xlsx`; `sheet.table(&TableSpec::new([...]))`
finds the header row among the first 10 non-empty rows (title rows above
are skipped), skips totals and notes rows below the data (at most two
cells and no key, by default the first column), and yields `Row`s with
their sheet name and 1-based row number. An Excel error cell is
`Cell::Unreadable` with its code and cell (`#REF!`, `C10`), never text.
`harpseal::sheet::number(text, Blank::Zero | Blank::Missing)` reads
`12,500`, ` 12500 ` and `12,500.00` as the same number; `.` is always the
decimal point. The rustdoc of `harpseal::sheet` has a worked example.
