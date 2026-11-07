---
status: accepted
date: 2026-11-07
---

# Backup format: a directory of NDJSON tables, a manifest, and FileStore objects, through the ports

`harpseal backup` writes a directory (format 1): `manifest.json` (Deployment name, `taken_at`/`finished_at`, the applied migrations in order with their checksums, each table's columns, row count and `CREATE` SQL, each FileStore object's key, size and SHA-256), one `tables/<table>.ndjson` per table (one JSON object per row, every column; integers as `42`, reals as `3.0`, text as strings, `null`, blobs as `{"$blob": "<base64>"}`), and `files/<sha256 of key>` for every object a `file_key` column names. It is read and written only through the `Db` and `FileStore` ports, so the same code and format serve rusqlite and D1. The `migration` table is not dumped: a restore re-runs the Deployment's migrations and refuses a backup whose migration list is not a prefix of the Deployment's.

We rejected copying the SQLite file (native only, no D1, and it ties a backup to the exact schema and file layout), `wrangler d1 export`-style SQL dumps as the format (dialect- and schema-coupled, and they replay `CREATE` statements the migrations already own), and one JSON document per table (does not stream, and one bad row spoils the table).

`restore --force` deletes the rows of every table in the backup, `audit_log`, `security_event` and `consent` included, in one batch that drops their append-only triggers and re-creates them from the saved SQL. This is the one exception to ADR-0007's "no code path deletes the audit log": the backup's own audit rows replace them, and the restore writes a `backup.restore` audit row with the manifest's SHA-256.

## Consequences

- Backups taken by one build restore into the same or a later build of the same Deployment; a column dropped or made `NOT NULL` without a default in a later migration makes old backups unrestorable until a migration path exists.
- The dump is not one transaction (the `Db` port has none for reads): the Deployment must be stopped during a backup.
- The backup carries secrets (password hashes, session ids) and personal data: it is as sensitive as the database.
- A format change bumps `format`; `restore` refuses formats it does not know.
