# Backups

This page covers the nightly backup Job, the Backups page and the Excel
data export. `harpseal backup` / `restore` by hand, and what a restore
does, are in [Self-hosting](self-hosting.md#backup-and-restore).

Every Deployment backs itself up once a night, on Workers and natively,
without stopping (#78):

```toml
[backup]
enabled = true            # default; false stops the schedule only
schedule = "0 2 * * *"    # cron, in utc_offset (default 02:00)
retention_days = 30       # default
max_table_mb = 24         # default: largest table, as NDJSON
max_table_rows = 150000   # default
max_export_cells = 150000 # default: "Download all data" limit
```

The Job `kernel.backup.run` writes the same backup format to the
FileStore under `backups/<UTC time>/` (`backups/20261107T190000000Z/manifest.json`, …):

- **Workers** (Workers Paid, as the rest of the repo: a step makes more
  than Free's 50 subrequests; a step stays near 400 of an invocation's
  1,000, which the up to 25 Jobs of one tick share, so at least 550 are
  left for the others): the R2 bucket bound as `FILES`
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
50,000 rows, written 32 MB, copied 200 files or made 350 database queries
and FileStore calls, saves where it stands
as `backups/<UTC time>.progress.json` and hands over to the next run a
minute later; a table that would overrun a run gets a run of its own. A
table is one entry built in memory (format 1 has no table parts), held
about three times over while it is written to R2, and read 500 rows per
D1 query, so before reading a table the Job counts its rows and estimates
its NDJSON size, and **fails the run naming the table** when it is over
`max_table_rows` (150,000: 300 queries, so the table fits one step) or `max_table_mb` (24 MB: about 72 MB of a Worker's 128 MB).
On Workers there is no fallback inside the Deployment for such a table
yet: the nightly backup keeps refusing until backups can split tables
(#88). Meanwhile back the database up with
`npx wrangler@4 d1 export <database> --remote --output backup.sql`, which
does **not** copy the files in R2 (copy those with R2's own tools). On a
native server, `harpseal backup` has no such limit, and both settings can
be raised freely.

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
- `restore` checks keys and references **before it writes anything**,
  every `UNIQUE` index included, partial ones too (a Student who unlinks
  and links again during the backup can leave two live Links in it): a
  backup that would not insert cleanly is refused with a report, never
  half-restored. Take the next night's backup then.

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
tables (the backup's table list) except the operational ones, with a header
row, audited `data.export` with the tables and the columns left out. It is
for reading, not for restoring. Left out by default, each still downloadable
alone from the page's table list: `audit_log`, `inbound_event`, `job`,
`outbound_message`, `rate_limit`. Every table has its own **Excel**
download there (`/admin/backups/export/<table>`, same Permission, same
audit). It is
built in memory within the request (`rust_xlsxwriter` has no streaming
mode on wasm32) at about 200 bytes per cell (measured natively: 100,000
rows × 10 columns peaked at 218 MB), so the page first counts the cells
and refuses (413) above `max_export_cells` (150,000 cells, about 30 MB on a
128 MB Worker), naming the largest table: download that one alone, or raise
the limit on a native server. A single table over the limit is refused the
same way (download a backup instead). A sheet stops at Excel's
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

## Back into D1

There is no direct restore into D1 yet. By hand, with the Worker stopped
(routes off, or `[triggers] crons = []` and no traffic). These steps are not
exercised by CI against a real D1: try them on a scratch database first.

1. Download the backup's `.tar` and unpack it (above).
2. Restore it natively into a new SQLite file, with the same build as the
   Worker: `HARPSEAL_DB=restored.sqlite3 HARPSEAL_FILES=restored-files
   harpseal restore 20261107T190000000Z`.
3. Dump it as SQL without the transaction lines D1 refuses, with
   foreign keys checked only at the end of the import (the dump lists
   tables by name, not parents first):
   `{ echo 'PRAGMA defer_foreign_keys = on;'; sqlite3 restored.sqlite3 .dump | grep -v -e '^BEGIN TRANSACTION' -e '^COMMIT' -e '^PRAGMA'; } > restore.sql`.
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
