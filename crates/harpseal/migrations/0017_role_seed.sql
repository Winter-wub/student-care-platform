-- Kernel migration 0017 (#110): `[roles]` is seeded once.
--
-- One row per (Role name, Permission) pair that a `migrate` has seeded from
-- `[roles]`, and one per Role name it has seeded (`permission_id` = '').
-- A later `migrate` grants only the pairs, and creates only the Roles, that
-- have no row here, so an Administrator's edit on `/admin/roles` (a
-- Permission taken away, a Role deleted) is never undone by a deploy.
-- Keyed by name, not `role.id`, and without a foreign key: the row must
-- outlive a deleted Role, so the Role is not re-created.
--
-- Starts empty, also on an existing database: the first `migrate` after
-- this one records every `[roles]` pair the database already holds without
-- writing it (`INSERT OR IGNORE`), and grants the others, as every
-- `migrate` before did. Backed up and restored like any other table.
CREATE TABLE role_seed (
    role_name TEXT NOT NULL,
    permission_id TEXT NOT NULL,
    seeded_at INTEGER NOT NULL,
    PRIMARY KEY (role_name, permission_id)
);
