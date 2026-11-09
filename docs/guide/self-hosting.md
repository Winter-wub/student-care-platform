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
refuses an empty secret). Outside dev mode the native binary still registers
the Fake Channel, but the Core refuses all its traffic (#133, see the
[Channels guide](channels.md#the-fake-channel-never-belongs-in-production)). With the Announcements Module on (the example has
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
from `[roles]` in `deployment.toml`, seeded once (each Role and each
Role/Permission pair on the first migrate that sees it; see
[the back office](back-office.md)), plus Roles created in
the database.

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
# seeded as admin@sealbay.example: roster import 01a1… (50 Students), 4 announcements created, 2 texts and 1 settings written
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
that Administrator and carries `"seed": true`; the one exception is the
fees bank account, saved as the system with `Deployment::save_setting_as_system` (no Administrator
holds `fees.bank_account.edit`). Running it again adds
nothing; it refuses a database that already has Students unless `--force`,
and it never creates Staff or writes a password. `just seed` runs
`migrate`, `staff create --admin` (the temporary password is printed once;
change it at first sign-in) and `seed-example` against `HARPSEAL_DB`.

### Dev mode (`HARPSEAL_DEV_ROUTES=1`)

For demos and `just e2e` only, never in production. The Deployment is built in
dev mode, so the Fake Channel takes traffic and Staff see it. Email is captured in
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

Set `public_url` (a top-level key of `deployment.toml`, before any
`[section]`) to the https origin the Deployment is served at, such as
`public_url = "https://students.example.ac.th"`; the examples also read
`HARPSEAL_PUBLIC_URL`, which wins. It must be an origin only (no path); a
trailing `/` is trimmed. LINE and Telegram fetch announcement images from it
(`/announcements/files/<token>`, public and unguessable, served only while
the announcement is sent and live), so without it chat Cards go out without
an image; the Mini App shows images either way. The path must reach the
Deployment from the internet without a sign-in.

Behind a reverse proxy, terminate TLS there, forward to `HARPSEAL_ADDR` and
set `[staff] trust_forwarded_for = true`. The Deployment backs itself up
every night into the FileStore; `harpseal backup` makes a point-in-time
copy by hand (below).

Webhooks are answered 200 as soon as the call is verified and its events
are claimed; the handling runs on a background task (`NativeBackground`), so
LINE or a proxy closing the connection no longer stops it half way (#97).
Events of one chat account are handled one at a time, in order; at most 16
events are handled at once across accounts. Time an event waits in that
queue eats into LINE's reply token (about a minute), so a backlog turns
Replies into metered Pushes (Quota); an event that waits over 60 seconds is
logged as `inbound.lost` and not answered. `serve` runs until the process
is killed; to let handling finish on shutdown, call
`serve_until(router, addr, shutdown)` with a future that completes on
SIGTERM: it stops taking connections and waits up to 8 seconds for the
background tasks. Anything a kill cuts off keeps an unfinished claim, which
the next process's tick logs as `inbound.lost` (ADR-0009).

## Backup and restore

```sh
cargo run -p harpseal-example-native --bin harpseal -- backup ./backups/2026-11-07
## table audit_log: 1240 rows
## ...
## backup written to ./backups/2026-11-07: 27 tables, 5310 rows, 3 files, 8 migrations (manifest sha256 9f1c…)
cargo run -p harpseal-example-native --bin harpseal -- restore ./backups/2026-11-07 \
    [--force] [--drop-sessions]
```

`just backup DIR` and `just restore DIR [--force] [--drop-sessions]` run the same against
`HARPSEAL_DB` and `HARPSEAL_FILES`.

`backup DIR` reads the database and the FileStore through the Deployment's
own ports and writes:

- `manifest.json`: format `2`, the Deployment name, `taken_at` and
  `finished_at` (ms UTC), the applied migrations in order, each table's
  columns, row count, `CREATE` SQL and its parts in order (each part's
  entry, row count and SHA-256), and each
  FileStore object's key, size and SHA-256 (so the manifest's own hash,
  recorded on restore, pins the whole backup);
- `tables/<table>.ndjson`, then `tables/<table>/00001.ndjson`, … for a
  table larger than one part (`[backup] part_rows`, `part_mb`): every row
  of every table (except `migration`), one JSON object per line, in key
  order across the parts, values typed (`42`, `3.0`, strings, `null`,
  blobs as `{"$blob": "<base64>"}`). `restore` reads format 2 and the
  format 1 of older builds (one `tables/<table>.ndjson` per table);
- `files/<sha256 of the key>`: every object a `file_key` column points at
  (today the roster files of `roster_import`). An object already gone from
  the FileStore is listed as `missing` with a warning; the backup still
  succeeds.

**Upgrading past #88:** `[backup] max_table_rows` and `max_table_mb` are
gone (tables are split into parts instead); a `deployment.toml` that
still sets them refuses to boot, naming the key: delete those lines, or
use `part_rows` / `part_mb`. A restore holds the whole backup in memory,
about 7 times its `tables/` size: see [Backups](backups.md).

It writes to `DIR.partial` and renames it to `DIR` at the end, so `DIR` only
ever holds a finished backup (`DIR/` is the same: the temporary
directory is always next to `DIR`, not in it). `DIR` must be new or empty.

**Stop the server first** for a point-in-time copy: the `Db` port has no
read transaction, so the backup reads table by table (children before
parents) and a write in between may or may not be in it. A backup taken
while the server runs, as the nightly one is, still restores: see
[Nightly backups](backups.md). The manifest
records when the dump started and finished.

**The backup directory is as sensitive as the database.** It holds Staff
password hashes, session ids, Students' personal data and the roster files,
so restored logins keep working. Keep it encrypted and access-controlled,
and delete old ones under the same retention as the data.

`restore DIR` loads a backup into a fresh, empty database: point
`HARPSEAL_DB` at a new file and do not run `migrate` first (it seeds
`[roles]`, and then the database is no longer empty). **Stop the server for
the restore too**, so no tick or request writes in between. It:

1. refuses a backup with migrations this Deployment does not have, checked
   per Module: each Module's migrations in the backup must be the first of
   that Module's here, in order and with the same checksums (another
   Deployment, or a newer build than the one restoring);
2. applies just the migrations the backup had, so the schema is the one
   the rows were dumped from;
3. refuses when a table or column of the backup does not exist, when a
   column the backup lacks is `NOT NULL` without a default, and when a
   table of the backup has a trigger that fires on `INSERT` (it would fire
   once per restored row);
4. checks every table entry and every file against its SHA-256 and row
   count in the manifest, and every file key;
5. refuses when any table has rows, unless `--force`; with it, also empties
   `role_seed` when the backup predates it, so the next `migrate` treats
   the restored Roles as an upgrade, not as edits;
6. puts each file back under its key (as `application/octet-stream`);
7. inserts the rows parents first; the last batch also writes the audit row
   `backup.restore` (manifest SHA-256, counts, what `--force` deleted),
   with `--drop-sessions` empties `staff_session` and `app_session`, and
   draws a new Menu revision salt (`menu_salt`) and drops the original's
   Menu re-link work (see below);
8. applies this Deployment's newer migrations, so data migrations added
   since the backup run over the restored rows.

`--force` first deletes the rows of every table in the backup, in one batch
that briefly lifts the append-only triggers of `audit_log`,
`security_event` and `consent` and puts them back unchanged; if anything in
that batch fails, nothing is deleted. The `backup.restore` audit row records
how many rows of each table were deleted and the id and time of the newest
audit row deleted (ADR-0008 is the one exception to the append-only audit
log). It keeps the schema, and tables that are not in the backup (a Module
added since, say) are left untouched; the command lists them. A restore that
fails while inserting leaves part of the backup behind: fix the cause and
run it again with `--force`.

**After a restore, before starting the server:**

- Jobs come back as they were: anything `pending` or due in the backup
  (announcement deliveries, roster imports, Quota emails) runs on the first
  tick. Look at `/admin/jobs` (or the `job` table) first and cancel what
  must not run again.
- Staff and App Sessions and password hashes come back as they were at the
  backup: a session or password revoked since is valid again. After a
  security incident restore with `--drop-sessions` (everyone signs in
  again) and reset the passwords that were changed since the backup.

- The Menu revision salt is new (#66). A copy restored from production
  (into staging, say) may run on the same LINE token: with the original's
  salt, its revisioned rich menus would carry the original's names and a
  retire could move or delete the original's. The restored Deployment
  therefore never retires rich menus named with the old salt; they stay
  until the next `menu_generation` bump or are deleted by hand in the LINE
  console. Its linked Students get rich menus under the new salt at their
  next Menu change. For the same reason the restore forgets the original's
  Menu re-link work: retires still pending on LINE (`menu_retire`, whose
  batch request ids belong to the original) are cleared, and queued or
  running `kernel.menu.relink` Jobs are marked `failed` ("cancelled by the
  restore of a backup"), so the copy never polls the original's batch nor
  bulk-links its users. A revision such a Job left `linking` is picked up by
  the next Menu label save.

**D1.** The CLI runs natively and has no D1 adapter yet. A Worker
Deployment is backed up by its nightly Job instead (next section), and
restored through a native restore; see [Back into D1](backups.md#back-into-d1).
Cloudflare's own export
(`npx wrangler@4 d1 export <database> --remote --output backup.sql`)
remains a second, independent copy.

## Nightly backups

Every Deployment also backs itself up each night into its FileStore, with
a Backups page to list, download and export: see [Backups](backups.md).
