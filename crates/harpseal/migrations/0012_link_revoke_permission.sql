-- Kernel migration 0012 (#80).
--
-- `kernel.link.manage` used to cover both creating a Manual Link and
-- revoking a Link. It now means Manual Link only; revoking (Staff unlink)
-- is `kernel.link.revoke`. Every Role that held `kernel.link.manage` before
-- this migration keeps what it could do, so it is granted the new
-- Permission too. Runs once: a Role edited afterwards (revoke taken away on
-- purpose) is not touched again. Permission ids are not rows of their own,
-- only `role_permission.permission_id` text.
INSERT OR IGNORE INTO role_permission (role_id, permission_id)
SELECT role_id, 'kernel.link.revoke' FROM role_permission
WHERE permission_id = 'kernel.link.manage';
