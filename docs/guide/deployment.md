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
no Mini App URL; a Menu label key with no text in the default Locale.

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
  request; Telegram 0). `usage_daily` adds them to `metered`, and Messages
  a Channel fails on to `failed` (a blocked-account skip counts nowhere).
- **Quotas (#26).** `/admin/usage` charts each Channel's month against
  `[quotas.<kind>]`; from 80 % every Staff page shows a banner (red at 100
  %) and the tick's `kernel.usage.watch` (every 10 minutes, only with
  `[quotas]`) emails active Administrators once per month per level.
- **Partial sends.** When a Channel stops part-way, what went out is
  recorded (`outbound_message` rows and usage; a Message cut off after some
  of its parts gets a row with `error = 'partly sent: …'`) and
  `Deployment::send_batch` returns `Error::PartialSend { sent, total, source
  }`. A Job resumes with `msgs[sent..]`; `Error::root()` gives the cause
  (`RateLimited`, `Unreachable`). Nothing before Message `sent` goes out
  again, but the parts of Message `sent` that were already delivered may
  be delivered twice, on any Channel: Telegram has no request
  de-duplication, and on LINE the resumed batch packs its requests
  differently, so their retry keys differ. That is the accepted trade-off:
  a repeated Card beats a lost one.
- **Idempotency keys.** `Deployment::send_batch_with_key(to, reply, msgs,
  locale, key)` passes a stable key (e.g. `<job id>/<account>`); LINE turns it
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
- **Resuming a partial send.** When the stopped Message opened its own Push
  requests, a resume with the same key rebuilds them with the same retry
  keys: LINE answers 409 to those that already went out, the receipt
  reports them in `SendReceipt::replayed_units`, the Kernel does not meter
  them again and completes the Message's "partly sent" row (matched by
  `outbound_message.batch_key`) instead of writing a second one. This is an
  approximation (#62): it over-counts a Message replayed whole whose first
  answer was lost, under-counts when recording the partial send failed,
  and a stopped Message that shared its first request with earlier ones
  gets those parts delivered twice.
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
