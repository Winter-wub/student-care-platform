-- Kernel migration 0019 (#165).
--
-- `kernel.setting.edit` used to cover the Texts page (`/admin/texts` and
-- `texts::set_override`). Texts now have their own Permission,
-- `kernel.texts.edit`, so a Role can edit texts without every setting.
-- Every Role that held `kernel.setting.edit` before this migration keeps
-- what it could do, so it is granted the new Permission too. Runs once: a
-- Role edited afterwards (texts taken away on purpose) is not touched
-- again. Permission ids are not rows of their own, only
-- `role_permission.permission_id` text.
INSERT OR IGNORE INTO role_permission (role_id, permission_id)
SELECT role_id, 'kernel.texts.edit' FROM role_permission
WHERE permission_id = 'kernel.setting.edit';
