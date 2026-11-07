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
set `[staff] trust_forwarded_for = true`. The Deployment backs itself up
every night into the FileStore; `harpseal backup` makes a point-in-time
copy by hand (below).

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

- `manifest.json`: format `1`, the Deployment name, `taken_at` and
  `finished_at` (ms UTC), the applied migrations in order, each table's
  columns, row count, `CREATE` SQL and the SHA-256 of its entry, and each
  FileStore object's key, size and SHA-256 (so the manifest's own hash,
  recorded on restore, pins the whole backup);
- `tables/<table>.ndjson`: every row of every table (except `migration`),
  one JSON object per line, values typed (`42`, `3.0`, strings, `null`,
  blobs as `{"$blob": "<base64>"}`);
- `files/<sha256 of the key>`: every object a `file_key` column points at
  (today the roster files of `roster_import`). An object already gone from
  the FileStore is listed as `missing` with a warning; the backup still
  succeeds.

It writes to `DIR.partial` and renames it to `DIR` at the end, so `DIR` only
ever holds a finished backup (`DIR/` is the same: the temporary
directory is always next to `DIR`, not in it). `DIR` must be new or empty.

**Stop the server first** for a point-in-time copy: the `Db` port has no
read transaction, so the backup reads table by table (children before
parents) and a write in between may or may not be in it. A backup taken
while the server runs, as the nightly one is, still restores: see
[Nightly backups](#nightly-backups-and-the-backups-page). The manifest
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
5. refuses when any table has rows, unless `--force`;
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
restored through a native restore; see [Back into D1](#back-into-d1).
Cloudflare's own export
(`npx wrangler@4 d1 export <database> --remote --output backup.sql`)
remains a second, independent copy.

## Nightly backups and the Backups page

Every Deployment backs itself up once a night, on Workers and natively,
without stopping (#78):

```toml
[backup]
enabled = true            # default; false stops the schedule only
schedule = "0 2 * * *"    # cron, in utc_offset (default 02:00)
retention_days = 30       # default
max_table_mb = 24         # default: largest table, as NDJSON
max_table_rows = 200000   # default
max_export_cells = 150000 # default: "Download all data" limit
```

The Job `kernel.backup.run` writes the same backup format to the
FileStore under `backups/<UTC time>/` (`backups/20261107T190000000Z/manifest.json`, …):

- **Workers** (Workers Paid, as the rest of the repo: a step makes more
  than Free's 50 subrequests): the R2 bucket bound as `FILES`
  (`[[r2_buckets]]` in `wrangler.toml`). Without that binding every
  night fails, and the Administrators are emailed.
- **Native:** the FileStore directory (`HARPSEAL_FILES`), so
  `HARPSEAL_FILES/backups/20261107T190000000Z` is a backup directory
  `harpseal restore` reads as it is (with `HARPSEAL_S3_BUCKET` set it goes
  to the S3 bucket instead). Copy that directory off the server too: a
  backup on the same disk does not survive the disk.

Backups sit next to the live files, in the same bucket or directory: give
the bucket the protection the data needs, and copy backups elsewhere when
the Deployment must survive losing it. Each backup holds a full copy of
the files, so the storage needed is about `retention_days` times the
database and files together; the Backups page shows the total.

**Steps and limits.** One Job run dumps whole tables until it has read
50,000 rows or written 32 MB (or copied 200 files), saves where it stands
as `backups/<UTC time>.progress.json` and hands over to the next run a
minute later; a table that would overrun a run gets a run of its own. A
table is one entry built in memory (format 1 has no table parts), held
about three times over while it is written to R2, and read 500 rows per
D1 query, so before reading a table the Job counts its rows and estimates
its NDJSON size, and **fails the run naming the table** when it is over
`max_table_rows` (200,000: 400 queries of a Worker invocation's 1,000
subrequests) or `max_table_mb` (24 MB: about 72 MB of a Worker's 128 MB).
A Deployment that large backs up natively with `harpseal backup` until
backups can split tables (follow-up ticket). On a native server, raise
both freely.

**Consistency while the Deployment runs.** D1 has no transactions, so the
tables are not one point in time: a row written during the backup may or
may not be in it. What the backup does promise:

- each table entry is pinned by its SHA-256 and row count in the manifest,
  and the manifest records `taken_at` and `finished_at`;
- a table is read in primary-key order, each page after the last key read,
  so a row deleted and re-inserted under the same key during the backup
  (a roster import replacing Tags, an edited announcement text) is in it
  once;
- tables are read children before parents, and the Kernel deletes no row
  that others reference except a Role (its grants go with it), so every
  reference resolves; grants of a Role deleted during the backup are
  dropped by the restore, as the delete would have done;
- `restore` checks keys and references **before it writes anything**: a
  backup that would not insert cleanly is refused with a report, never
  half-restored.

**Retention.** After each successful backup, runs started more than
`retention_days` ago (finished or failed) are deleted with their objects,
at most 20 per night, each audited `backup.prune`. The newest successful
backup is never deleted, and nothing is deleted after a failed night. A
restore never writes `backup_run` (the list of backups): the target keeps
its own, so backups taken after the one restored stay listed and pruned.
A copy restored into a new Deployment starts with an empty list: give it
its own bucket or directory.

**Failures.** A failed step is retried like any Job (1, 5 and 30 minutes
later). After the last retry, or when a run makes no progress for two hours
(a Worker killed mid-step), the run is marked failed, the Backups page
shows the error, and every active Administrator (Staff whose Roles grant
`kernel.staff.manage`) gets one email per failure day (audited
`backup.notify`; needs an email provider). A retention failure after a
good backup is shown on that backup's row and emailed separately (once a
day too).

**The Backups page** (`/admin/backups`) has two Permissions, which no Role
holds until you grant them:

- `kernel.backup.manage`: the list (time, status, size, contents, errors,
  total storage) and **Back up now** (refused while a backup runs,
  audited `backup.request`);
- `kernel.backup.download`: **Download (.tar)** and **Download all data**.
  **This is Administrator-equivalent**: a backup holds every Student's
  data and the Staff password hashes (offline-crackable), so grant it only
  to Administrators.

**Download (.tar)** streams one finished backup object by object (audited
`backup.download`). It unpacks to the backup directory:
`tar -xf backup-20261107T190000000Z.tar`, then
`harpseal restore 20261107T190000000Z`.

**Download all data (Excel)**: one sheet per table, Kernel and Module
tables (the backup's table list), with a header row, audited `data.export`
with the columns left out. It is for reading, not for restoring. It is
built in memory within the request (`rust_xlsxwriter` has no streaming
mode on wasm32) at about 200 bytes per cell (measured natively: 100,000
rows × 10 columns peaked at 218 MB), so the page first counts the cells
and refuses above `max_export_cells` (150,000 cells, about 30 MB on a
128 MB Worker); raise it on a native server. A sheet stops at Excel's
1,048,575 rows (the audit row names the tables cut), long texts at 32,767
characters, and blobs show as `[N bytes]`.

Columns left out of the Excel export as secrets, by name in every table
(name a Module's secret columns the same way):

| Rule | Kernel columns it drops today |
|---|---|
| starts with `password` | `staff.password_hash` |
| `hash` or ends with `_hash` | `verification_challenge.secret_hash` |
| contains `token` | `job.lock_token`, `menu_retire.token` |
| contains `salt` | `menu_salt.salt` |
| contains `secret` | (none besides the above) |
| `id` of a `*_session` table (the hash of the session cookie) | `staff_session.id`, `app_session.id` |

The audit log's `before`/`after` are already redacted when written. Live
Verification challenges (`pending`, not yet expired) are left out of
backups altogether: the salted hash of a short code is crackable offline.

### Back into D1

There is no direct restore into D1 yet. By hand, with the Worker stopped
(routes off, or `[triggers] crons = []` and no traffic). These steps are not
exercised by CI against a real D1: try them on a scratch database first.

1. Download the backup's `.tar` and unpack it (above).
2. Restore it natively into a new SQLite file, with the same build as the
   Worker: `HARPSEAL_DB=restored.sqlite3 HARPSEAL_FILES=restored-files
   harpseal restore 20261107T190000000Z`.
3. Dump it as SQL without the transaction lines D1 refuses:
   `sqlite3 restored.sqlite3 .dump | grep -v -e '^BEGIN TRANSACTION' -e '^COMMIT' -e '^PRAGMA' > restore.sql`.
4. Create a new, empty D1 database
   (`npx wrangler@4 d1 create harpseal-restored`) and load it:
   `npx wrangler@4 d1 execute harpseal-restored --remote --file restore.sql`.
   It carries the `migration` table, so the Worker applies only newer
   migrations.
5. Put the files back into R2 from `restored-files/`, one object per file,
   under the same key: `npx wrangler@4 r2 object put
   <bucket>/<key> --file restored-files/<key> --remote`.
6. Point `database_id` in `wrangler.toml` at the new database, deploy, and
   turn routes and the cron back on.
