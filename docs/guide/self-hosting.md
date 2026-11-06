# Self-hosting with the native runtime

Running a Deployment as one binary on your own server: its settings, the
`harpseal` CLI, dev mode and the demo seed. Back to the
[README](https://github.com/Winter-wub/student-care-platform#readme).

### Running the example natively

```sh
FAKE_SECRET=change-me cargo run -p harpseal-example-native
## migrations: 8 applied, 0 already done
## listening on http://127.0.0.1:8080  (try /notice)
```

Every setting comes from the environment (`NativeSettings::from_env`);
secrets never go in TOML. Empty values count as unset. Building the native
runtime compiles `aws-lc-sys` (rustls' crypto for reqwest and lettre), which
needs a C compiler, plus NASM on Windows x86-64.

| Variable | Default | Meaning |
|---|---|---|
| `HARPSEAL_DB` | `harpseal.sqlite3` | SQLite file (created and migrated at start) |
| `HARPSEAL_ADDR` | `127.0.0.1:8080` | listen address |
| `HARPSEAL_FILES` | `files` | FileStore directory |
| `HARPSEAL_S3_BUCKET` | unset | use an S3-compatible bucket instead of `HARPSEAL_FILES` |
| `HARPSEAL_S3_ENDPOINT` | required with a bucket | `https://<account>.r2.cloudflarestorage.com`, `https://s3.<region>.amazonaws.com`, MinIO, ... (path-style) |
| `HARPSEAL_S3_REGION` | `auto` | SigV4 signing region |
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` | required with a bucket | S3 credentials |
| `HARPSEAL_EMAIL` | `smtp` | `smtp` or `resend` |
| `HARPSEAL_SMTP` | `smtp://localhost:25` | lettre URL: `smtp://host:25` (unencrypted), `smtp://host:25?tls=opportunistic` (STARTTLS if offered), `smtp://user:pass@host:587?tls=required`, `smtps://user:pass@host:465`; credentials in `smtp://` without `tls=required`/`opportunistic` are refused |
| `RESEND_API_KEY` | required with `resend` | Resend API key |
| `HARPSEAL_EMAIL_FROM` | `noreply@localhost` | sender address (`Name <addr@domain>`) |

The Deployment's secrets: `LINE_CHANNEL_SECRET`, `LINE_ACCESS_TOKEN`,
`LINE_LIFF_ID`, `TELEGRAM_BOT_TOKEN`, `TELEGRAM_WEBHOOK_SECRET`,
`TELEGRAM_WEB_APP_URL`, `FAKE_SECRET` (required, non-empty: the Fake Channel
refuses an empty secret). With the Announcements Module on (the example has
it), `LINE_LIFF_ID` and `TELEGRAM_WEB_APP_URL` are required too: its Cards
carry Mini App buttons, and `build()` refuses a Channel without a Mini App.
A timer calls `Deployment::tick` every minute, each tick in its own task: a
panicking Job handler is recorded as a failed run of that Job, and any other
panic in a tick is logged and the next tick still runs. The plain `release` profile keeps panic unwinding for this binary;
the Worker builds with `wasm-release` (release + `panic = "abort"`).

#### Back office and the `harpseal` CLI

Create the first Administrator against the same database, then sign in at
<http://127.0.0.1:8080/admin/login>:

```sh
cargo run -p harpseal-example-native --bin harpseal -- migrate
cargo run -p harpseal-example-native --bin harpseal -- \
    staff create --admin --email admin@sealbay.example
## created staff admin@sealbay.example (0190...)
## temporary password (shown once, change it at first sign-in): 839c9e-c8b47a-1015c8-ad0fe8
```

`--password-stdin` reads the password from standard input instead (at least 12
characters; it is then not temporary). `--admin` is refused once any Staff
exists unless `--force` is given; `--role NAME` grants other Roles. Roles come
from `[roles]` in `deployment.toml`, merged into the database on every migrate
(add-only), plus Roles created in the database.

Sessions are 12 hours sliding and 7 days absolute; 5 wrong passwords in a row
lock the account for 15 minutes. The session cookie is `Secure`: browsers
accept it on `http://localhost`, but to test from another device over plain
http set `[staff] cookie_secure = false` in a local copy of the TOML (never in
production). Every response carries an `x-request-id`, which audit rows record.
Audit rows, sessions and Security Events take the client address from the TCP
peer natively and from `CF-Connecting-IP` on Workers; behind your own reverse
proxy set `[staff] trust_forwarded_for = true` to use the right-most public
`X-Forwarded-For` hop instead. A password change ends every session of that
Staff member and starts a fresh one; the per-minute tick deletes expired
sessions. `--admin` on an `Administrator` Role that already exists from
`[roles]` keeps the TOML's Permissions and prints the ones it lacks.

### Demo data: `harpseal seed-example`

The example's CLI has one command of its own. After `staff create --admin`
it writes the demo through the audited paths ([Building a
Deployment](deployment.md#writing-from-code-the-same-audited-paths-as-the-back-office)),
acting as the oldest active Administrator (or `--as EMAIL`):

```sh
cargo run -p harpseal-example-native --bin harpseal -- seed-example [--as EMAIL] [--force]
# seeded as admin@sealbay.example: roster import 01a1… (50 Students), 4 announcements created, 2 texts written
```

**Demo databases only.** While it waits for the roster import it runs
`Deployment::tick` in the CLI, which runs every due Job there, with no Fake
Channel and the real EmailSender: on a live database pending announcement
deliveries would run from the CLI and Quota emails could go out.
`seed-example --force` prints a warning to that effect; never point it at
production.

It imports `deployments/example/data/roster.csv` (50 fictional Students,
upsert mode, applied at once by running the import Jobs), creates three
draft announcements and one scheduled a week ahead, and overrides
`kernel.flow.welcome` in Thai and English, unless that text already has an
override (an Administrator's customisation is never overwritten). Every audit row it writes names
that Administrator and carries `"seed": true`. Running it again adds
nothing; it refuses a database that already has Students unless `--force`,
and it never creates Staff or writes a password. `just seed` runs
`migrate`, `staff create --admin` (the temporary password is printed once;
change it at first sign-in) and `seed-example` against `HARPSEAL_DB`.

### Dev mode (`HARPSEAL_DEV_ROUTES=1`)

For demos and `just e2e` only, never in production. Email is captured in
memory instead of sent, and three routes answer only with header
`x-dev-secret: <FAKE_SECRET>` (404 otherwise):

| Route | Answer |
|---|---|
| `GET /dev/outbox?to=<external id>` | JSON: what the Fake Channel sent that account, and its Menu (`unset`, `locked` or the entries) |
| `GET /dev/email?to=<address>` | JSON: the last email to that address (the OTP is in the subject) |
| `POST /dev/tick` | runs `Deployment::tick` now instead of waiting for the minute timer |

The OTP is not weakened: same length, expiry and attempt limit. Dev mode
refuses to start unless `HARPSEAL_ADDR` is a loopback address (`127.x`,
`::1`, `localhost`) and `FAKE_SECRET` has at least 16 characters and is not
the `change-me` placeholder; the error names the rule. Captured emails are
kept in memory (the last 50) and only "captured email to <address>" is
logged, never the subject (it holds the OTP). `just run` starts the example
in dev mode with dummy Mini App ids.

## Hosting your own Deployment

A customer Deployment is its own repository (ADR-0001) with the same shape
as `deployments/example`: a library crate holding `deployment.toml`, its
Modules and a `builder()`, a native binary and/or a Worker, and tests that
boot it on `harpseal-testing`. Depend on the published crates (or on a
`git`/`path` dependency while developing) and pin a released version before
handing over. Put Students' data mapping in `[roster]`, Roles in `[roles]`,
texts in `[texts.<locale>]`; secrets only in the environment.

Behind a reverse proxy, terminate TLS there, forward to `HARPSEAL_ADDR` and
set `[staff] trust_forwarded_for = true`. Back up the SQLite file (and the
files directory) with the server stopped or with SQLite's online backup;
backup and restore commands in the CLI are planned in a follow-up ticket.
