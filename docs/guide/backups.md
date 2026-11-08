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
part_mb = 8               # default (at most 32): a table part ends past this size
part_rows = 20000         # default: a table part ends at this many rows
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

The files are those a row names: a `file_key` or `*_file_key` column, and
(since #114) the three branding settings saved on `/admin/branding`
(`kernel.branding.logo_file_key`, `.menu_locked_file_key`,
`.menu_unlocked_file_key`, Locale `''`) when their value has the shape the
Kernel writes (`branding/<sha256>.png|jpg`, `branding/menu/<sha256>.png`).
Any other `setting` is data: a Module setting named `..._file_key`, or a
branding value pointing elsewhere, is backed up as a row and never followed
as a file, so it can neither break the backup nor pull other objects in. The format is unchanged (still 2): they are `files/<sha256>` entries
like any other, listed in the manifest, and a restore puts them back. A
backup taken before #114 restores as before.

**Steps and limits.** One Job run reads tables 500 rows per D1 query
until it has read 50,000 rows, written 32 MB, copied 200 files or made
350 database queries and FileStore calls, saves where it stands as
`backups/<UTC time>.progress.json` and hands over to the next run a
minute later. **Any table size backs up** (#88, backup format 2): a
table is written in parts, `tables/<table>.ndjson`, then
`tables/<table>/00001.ndjson`, `00002`, …, each ending at `part_rows`
rows (20,000: 40 queries), once it passes `part_mb` (8 MB, by about one
row: pages shrink to the bytes left once a row's size is known), or
where a run's budget ran out; the next run reads on after the last key
of the last part. One part at a time is built in memory, held about three
times over while it is written to R2, so a run stays well inside a
Worker's 128 MB whatever the table's size. Raise `part_mb` on a native
server if you want fewer, larger parts (at most 32); on Workers keep it
at 8 or below. Each part lists the files its own rows name (copied once
the tables are done). Before it writes a part, a run saves its progress
naming that part, so a run that dies (and is retried, or fails and is
pruned) leaves no part behind that nothing names.

**Consistency while the Deployment runs.** D1 has no transactions, so the
tables are not one point in time: a row written during the backup may or
may not be in it. What the backup does promise:

- each table part is pinned by its SHA-256 and row count in the manifest,
  in order, and the manifest records `taken_at` and `finished_at`;
- a table is read in primary-key order, each page after the last key read,
  so a row deleted and re-inserted under the same key during the backup
  (a roster import replacing Tags, an edited announcement text) is in it
  once;
- tables are read children before parents, and the Kernel deletes no row
  that others reference except a Role (its grants go with it), so every
  reference resolves; grants of a Role deleted during the backup are
  dropped by the restore, as the delete would have done;
- a table read over several runs, minutes apart, is checked before the
  backup moves on: when its rows are all written, its keys on every
  `UNIQUE` index (partial ones too) are compared across all its parts.
  A Student who unlinks and links again between two runs would leave two
  live Links in the backup; the part holding the stale row is then read
  again over its key range, as the database has it now, until no key
  repeats. Each row's key is kept as an 8-byte hash per index, saved
  between runs under `backups/<UTC time>.check/` (one object per run
  that stopped inside the table, so the check reads one object per run,
  not one per part) and deleted when the table is done. At its peak the
  check holds about **16 bytes per row per `UNIQUE` index** (the hashes
  and a sorted copy of one index's), so a Worker's 128 MB takes a table
  of roughly 4 to 5 million rows per index. If keys still repeat after
  three re-reads, the whole keys are compared (a hash collision passes)
  and a real repeat fails the run with the table and index named
  (retried, then emailed). A part re-read must stay near a part: when
  more than twice `part_rows` rows (or twice `part_mb`) were written
  into its key range meanwhile (a large roster import under keys that
  are not in time order, say), the run fails naming the table rather
  than hold them all in memory; the next night's backup starts over. So
  **a backup marked done restores** as far as the Kernel's tables go: a
  Module table whose foreign key is `ON DELETE SET NULL` (or `RESTRICT`,
  or none) can still hold a child of a row deleted during the backup,
  which the restore refuses, as it drops only `ON DELETE CASCADE`
  orphans;
- `restore` checks keys and references again **before it writes
  anything**: a backup that would not insert cleanly (hand-edited, or
  from an older build) is refused with a report, never half-restored.

**Progress size.** The saved progress lists every part written so far
with the FileStore keys its rows name, and every object copied: it grows
with the number of parts and of objects, as the manifest does (each key
once per part that names it), and is rewritten before each new part.

**Restore memory.** `harpseal restore` runs natively only (never in a
Worker) and holds every row of the backup in memory while it checks keys
and references before writing: plan on about **7 times the size of the
`tables/` entries** (measured: 32 MB of NDJSON, 400,000 rows, took
about 210 MB more at its peak, an in-memory database included), plus the
largest file. A backup with 1 GB of tables wants a machine with 8 GB.

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
Kept Module uploads (`module_upload`, a spreadsheet waiting for its
confirm, #74) are left out too, rows and files: they live 24 hours (a week
once confirmed) and a backup would otherwise keep them for 30 days.

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
