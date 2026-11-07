-- Kernel migration 0011 (#66).
--
-- `menu_revision`: the Menu revisions of each effective Locale. A
-- back-office save of a Menu label inserts the next revision (and, the
-- first time, revision 0 for the Menu shown until then), and enqueues the
-- Job `kernel.menu.relink`, which re-links every linked account of that
-- Locale to the newest revision and only then retires the older ones.
-- `entries` is the Menu of that revision as JSON (`[{label, action}]`),
-- so the Job can name the platform object of a revision it did not build.
-- `state`: `linking` until the Job re-linked every account, `live` after,
-- `retired` once its platform object is gone. `owner` is the job id that
-- claimed the re-link of this revision, so a second Job for the same
-- revision leaves it alone.
CREATE TABLE menu_revision (
    locale TEXT NOT NULL,
    revision INTEGER NOT NULL,
    entries TEXT NOT NULL CHECK (json_valid(entries)),
    state TEXT NOT NULL CHECK (state IN ('linking', 'live', 'retired')),
    owner TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (locale, revision)
);
