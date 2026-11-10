-- Announcements Module (#144): saved Audiences.
--
-- Staff name an Audience on the announcement editor and pick it again
-- later. A row keeps the rules, not the Students they selected: the
-- choice (`scope`: everyone, groups, students, or '' for a combined
-- Audience saved before #120) and the filters (`audience`, the JSON of
-- `crate::audience::Audience`: attributes, Tags, hand-picked student ids,
-- include unreachable) with `"v": 1`, the version of that JSON: a row
-- that does not read as version 1 is shown as unreadable and only deleted,
-- never read as everyone. Picking one fills the editor; the reach is counted
-- again on the editor and on the review step, never taken from here.
--
-- Names are unique, so the picker never shows two alike.
CREATE TABLE saved_audience (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    scope TEXT NOT NULL CHECK (scope IN ('', 'everyone', 'groups', 'students')),
    audience TEXT NOT NULL,
    created_by TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
