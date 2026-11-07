---
status: accepted
date: 2026-11-07
---

# Backup format: a directory of NDJSON tables, a manifest, and FileStore objects, through the ports

`harpseal backup` writes a directory (format 1): `manifest.json` (Deployment name, `taken_at`/`finished_at`, the applied migrations in applied order with their checksums, each table's columns, row count, `CREATE` SQL and the SHA-256 of its entry, each FileStore object's key, size and SHA-256), one `tables/<table>.ndjson` per table (one JSON object per row, every column; integers as `42`, reals as `3.0` (exact round trip), text as strings, `null`, blobs as `{"$blob": "<base64>"}`), and `files/<sha256 of key>` for every object a `file_key` column names. The manifest pins every entry, so its own SHA-256 (recorded in the restore's audit row) identifies the whole backup. It is read and written only through the `Db` and `FileStore` ports and the async `BackupSink`/`BackupSource` traits, so the same code and format serve rusqlite and D1 (and could be stored in R2 or S3). The `migration` table is not dumped: a restore checks, per Module, that the backup's migrations are the first of that Module's in the Deployment, applies only those, inserts the rows, then applies the newer migrations so data migrations run over the restored rows.

We rejected copying the SQLite file (native only, no D1, and it ties a backup to the exact schema and file layout), `wrangler d1 export`-style SQL dumps as the format (dialect- and schema-coupled, and they replay `CREATE` statements the migrations already own), one JSON document per table (does not stream, and one bad row spoils the table), and checking the migration list as one global prefix (the applied order interleaves Modules when a Kernel migration ships after a Module's, so a Deployment's own backup would be refused).

`restore --force` deletes the rows of every table in the backup, `audit_log`, `security_event` and `consent` included, in one batch that drops their append-only triggers and re-creates them from the saved SQL. This is the one exception to ADR-0007's "no code path deletes the audit log": the backup's own audit rows replace them, and the `backup.restore` audit row, written in the same batch as the last restored rows, records the manifest's SHA-256, the rows deleted per table and the id and time of the newest audit row deleted.

## Consequences

- Backups taken by one build restore into the same or a later build of the same Deployment; a migration that drops a column or adds a `NOT NULL` column without a default still applies after the rows go in, but a database already ahead of the backup is checked column by column and may refuse.
- The dump is not one transaction (the `Db` port has none for reads): stop the Deployment for a point-in-time backup and for every restore. Online backups are allowed since #78 (see the amendment below).
- The backup carries secrets (password hashes, session ids) and personal data: it is as sensitive as the database. `--drop-sessions` ends restored sessions.
- A backed-up table with an `INSERT` trigger is refused on restore (the trigger would fire per row); a Module that needs one must change this rule first.
- A format change bumps `format`; `restore` refuses formats it does not know.

## Amendment (#78, 2026-11-07): online, chunked backups

The nightly backup Job dumps format 1, unchanged, while the Deployment runs, over several Job runs (a saved resume point next to the backup). The format is the same; what changed is how a dump reads and what a restore checks:

- **Keyset paging.** A table is read in primary-key order, each page after the last key read (`rowid` only for a table without a primary key, which the Kernel does not have). A row deleted and re-inserted under the same key mid-dump is read at most once, so no entry holds a duplicate key. An entry's rows are therefore in primary-key order, no longer `rowid` order; a restore never relied on the order.
- **Children-first order.** Tables are dumped children before parents, so a row added mid-dump never references a parent missing from the backup. The Kernel deletes no referenced row except a Role, whose grants cascade.
- **Pre-write validation.** Before it writes anything, a restore checks that no two rows share a primary or non-partial `UNIQUE` key and that every foreign key resolves among the backup's rows. Rows orphaned through an `ON DELETE CASCADE` key are dropped (the delete would have removed them) and counted in the audit row; any other problem refuses the restore with a report.
- **Per-table size limit.** A table is one entry built in memory. The Job counts rows and estimates the NDJSON size before reading a table and fails the run, naming it, above `[backup] max_table_rows` (default 200,000) or `max_table_mb` (default 24, for a 128 MB Worker). Such a Deployment backs up natively with `harpseal backup`. **Follow-up:** table parts (a table split over several entries) would lift this; that is a format change (`format: 2`).
- **Left out.** Live Verification challenges are never dumped, and a restore never writes `backup_run` (the target's own list of backups).
