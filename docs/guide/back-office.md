# The back office

What Staff see under `/admin`, and the Permission each page needs. Back to
the [README](https://github.com/Winter-wub/student-care-platform#readme).

`/admin/students` lists the roster (50 per page, HTMX search by name or code)
and opens a Student: fields, attributes, Links per Channel, Tags, Consent and
recent Security Events. A Student with **missing data** (no email, an email that
is not a valid address, or a `[roster.attributes]` attribute with no value) has a
"ข้อมูลไม่ครบ" badge naming what is missing, and the checkbox "only Students with
missing data" (`?missing=1`, kept across pages and search) lists just them. The
roster import warns about the same things (below). The filter reads the Student
table about three times per page (D1 bills per row read), fine for an occasional
Staff action; it does not store a flag. One email check serves the roster, the edit
form (a Thai or `a@x..example` address is refused there too) and this badge.
In the chat, a Student cannot start onboarding, and is told to contact Staff for a
Manual Link, when their email is missing, not valid, or the same (any case) as
another Student's who is not left; the last is recorded as a Security Event
`link.shared_email` (at most once an hour per chat account and Student; its details
name the other Students' codes in `shared_with`), since whoever reads that mailbox
could link as either. A resend runs the same checks again, so a code never goes to
an address an import or an edit has since made invalid or shared; the waiting claim
is cancelled. The lookup uses an index on `lower(trim(email))` (migration 0016);
emails are stored trimmed of every Unicode space so SQL and Rust compare the same. Staff with `kernel.student.edit` edit fields (each
edited field is kept by roster imports until "ใช้ค่าจากทะเบียน"), manual Tags and
the status. Staff with `kernel.link.manage` create Manual Links; Staff with
`kernel.link.revoke` unlink a Student's chat account. The two are separate, so
a Role can hold either (Student Affairs may unlink without being able to link
any account to any Student), and the detail page shows each action only to
Staff who hold its Permission. `kernel.student.edit` can still remove every
Link of a Student, by marking them left.

Before the split `kernel.link.manage` covered both. `migrate` grants
`kernel.link.revoke` once to every Role in the database that holds
`kernel.link.manage`. A `[roles]` entry that lists `kernel.link.manage` without
`kernel.link.revoke` is seeded with both, and the migrate report prints a notice
for it on every `migrate` (on Cloudflare, every cold start); update the entry to
say what you mean. An entry that lists `kernel.link.revoke` is taken as
written, so list it alone for unlink without Manual Link.

`[roles]` is seeded once. A `migrate` creates a Role name and grants a
(Role, Permission) pair only the first time it sees it, and records what it
seeded in `role_seed` (backed up and restored with the rest). So a Permission
added to `[roles]` later is granted on the next `migrate`, but one an
Administrator removed on the Roles page stays removed, and a Role deleted
there is not re-created; only a Role name new to `[roles]` is. Each grant the
seed really makes is audited (`role.create` or `role.update`, actor
`system`), except on a new Deployment's first `migrate`, which installs the
Roles into an empty database. Removing a Permission or a Role from `[roles]` changes nothing in
the database: do that on the Roles page. A Role created on the Roles page before
`[roles]` lists its name gets that entry's Permissions on the next `migrate`.
A Module removed and later added back does not re-grant its Permissions to
Roles seeded with them before; any taken away meanwhile, grant again on the
Roles page. The first `migrate` of a database
created before this rule records every `[roles]` pair it already holds, and
grants the ones it lacks, as every `migrate` before did.

The attributes come from `[roster.attributes.<key>]` in
`deployment.toml`: `column`, `type` (`text`, `integer`, `number`, `date` or
`enum` with `values`), `label = { th = "..", en = ".." }`, `optional`,
`shown_in_list`, `student_visible` (default `false`: shown to the Student
on a Module's Mini App page only when `true`) and `order` (see
`harpseal::RosterConfig`).

An `integer` or `number` attribute can set `min` and `max` (inclusive; either
one alone is fine): `min = 1`, `max = 6` for a year level. A value outside the
range is a **blocking** problem of the import (row, column and the range in the
report) and the Student's edit form refuses it with the same message; only the
fields Staff change are checked, so a Student already stored outside a newly
set range can still be edited. An `enum` attribute can give each value a
display label per Locale:

```toml
[roster.attributes.status]
column = "สถานะ"
type = "enum"
values = ["current", "graduated"]
value_labels = { current = { th = "กำลังศึกษา", en = "Studying" }, graduated = { th = "จบการศึกษา" } }
```

Staff read the label (in their own Locale, else the default Locale's, else the
value) on the Students list and Student page, the edit form's select, the
import page's list of what a cell may hold, the import report (the allowed
values, each as `label (value)`) and the Audience form and its summary. Stored
data, the Audience filter and a Module's Mini App page keep the **value**. A
roster cell may hold the value or the label in the Deployment's default Locale
(exact after trimming; ASCII letters in either case, so `studying` matches
`Studying`); the Student is stored with the value. A cell that is a value is
never read as a label. A label in another Locale is not read, which is why the
import page and report list the default Locale's labels, whatever Locale the
Staff member reads. The match is exact after trimming: there is no Thai
normalisation, so a zero-width character or a decomposed sara am in the cell
will not match and is reported as not in the list. The Deployment
does not build if `min` is above `max`, `min` / `max` sit on a type that is not
a number (or are not whole numbers on an `integer`), `value_labels` sit on a
type that is not an `enum`, name a value that is not in `values`, are empty,
name a Locale that is not configured (`th`, `en`, the default Locale or a
`[texts.<locale>]` table; `TH` is a typo), or make two values show the same
text in a Locale (a value shows its label, else the default Locale's, else the
value itself, so a label equal to an unlabelled value's text clashes), or, in
the default Locale, equal another value. Configs without these keys behave as
before, and any other key is refused.

`/admin/roster` imports the roster (`kernel.roster.view` to see imports,
`kernel.roster.import` to upload). Upload a `.csv` (UTF-8, `,` or `;`; a
`.txt` is refused) or `.xlsx` whose headers match `[roster]` exactly (the page
lists them and offers `template.csv`), choose `full` (Students missing from the
file become left and lose their Links) or `upsert` (add and update only;
nobody becomes left). The form preselects `[roster] default_mode` (default
`full`), and a request that sends no mode uses it too. A Deployment whose files
never list every Student (one file per cohort) sets `modes = ["upsert"]` and
`default_mode = "upsert"`: the form then offers only `upsert`, and a request
asking for `full` is refused with 400 and imports nothing (`modes` must not be
empty, hold only `full` / `upsert` and include `default_mode`; a bad value stops
the Deployment from building). The preview and the import page name the mode.
A **blocking** problem refuses the
whole file with a CSV report: an empty or malformed student id, a duplicate id in
the file, a required value missing or off its type, list or range, an unreadable value,
data in a column without a header, a hidden roster sheet in `full` mode. A
**warning** does not: the row imports and the preview lists it (sheet, row, column
and a count per class, with the same list in `report.csv`, which gains a level
column). The warning classes, all on by default: `missing_email` (the Student
cannot start onboarding until Staff add one), `invalid_email` (imported as typed,
so Staff can fix it on the Student's page), `duplicate_email` (the address is used by
another Student in the file, or by one who keeps it after the import: not left, not
a `full` import's leavers, not given another address by the file; a row whose email
Staff set by hand is not compared) and `empty_attribute` (an optional attribute with
no value: one warning per column and sheet, with the count and the first rows). A
Deployment makes classes blocking with `[roster] blocking = ["invalid_email",
"duplicate_email"]` (only these four names are accepted; anything else stops the
Deployment from building). A file with only warnings imports after Confirm, and
the warnings stay on the import's page. Older imports keep their reports; a bad
email used to refuse the file.
An `.xlsx` is read sheet by sheet (one sheet per cohort is fine; at most 50
sheets). **Every visible sheet with content must hold the header**: a
workbook with an extra notes or lookup sheet that imported before #70 is now
refused, naming that sheet; delete or hide it. Hidden sheets and empty
sheets are skipped (hidden ones are listed); a workbook whose content is
only in hidden sheets is refused naming them, and a hidden sheet whose
header holds the student id column (even an old tab missing another column)
refuses a `full` import (its Students would become left:
unhide or delete it; `upsert` lists it as skipped). In each sheet the header
row is found among its first 10 non-empty rows, so title rows above it are
fine. A totals or notes row below the data is skipped only when its student
id cell holds a label (text with letters, such as `รวม` or a note) and the
other columns are empty except the `integer` and `number` attributes; a row
with a name but no id, a row with only numbers, an id like `6801 0002`, or
an Excel error stays data and is reported. A sheet whose rows under the
header would all be skipped this way is refused, unless they hold no number
but zero (`รวม | 0` under an empty cohort's header is an empty sheet).
Skipped rows and sheets are
listed on the preview and in `report.csv`, with sheet, row and text.
Numbers may be stored as text (`12,500`, `12,500.00`; a lone `-` counts as
empty; exponents such as `1e3` are refused). A calculated cell (`=AVERAGE`)
reads as Excel shows it, a fractional part rounded to 15 significant
digits; a whole number stays exact, and a number cell of
9,007,199,254,740,992 or more cannot be read exactly and is refused as
unreadable (type long ids as text). An Excel error cell
(`#REF!`, `#N/A`) is refused as unreadable. The report names the sheet, row
and cell of each problem; a student id repeated in two sheets is a
duplicate.
Emails must be ASCII (letters, digits and symbols; an address with Thai
characters is an `invalid_email`). A preview shows new / updated / unchanged /
returning / fields kept by manual edits and, for `full`, will become left /
kept active because Staff set the status; Confirm runs the Job
`kernel.roster.import` in 500-row chunks (the page polls its progress), and the
done page shows each preview number next to what was imported. One import at a
time; the file stays in the FileStore under `roster/<import id>/` (its parsed
rows under `roster-rows/<import id>.jsonl`) for one year, then the per-minute
tick deletes both (audit `roster.file.prune`); the import's page and report
stay.
Try it with `deployments/example/data/roster.csv` (or `.xlsx`).

`/admin/announcements` (`harpseal::modules::announcements::Announcements`;
`announcements.view`, `.send`, `.retract`) creates an announcement (Thai text,
English optional), picks an Audience (roster attributes, any of their values,
AND Tags, AND Link status) with a live count and the metered Push estimate, and
sends it now or at a set time: Job `announcements.deliver` snapshots one
delivery row per live Link and starts one `announcements.deliver.channel` per
multicast group of 500 (LINE) or 25 accounts (Telegram, paced). The Card has
"รับทราบ" (`announcements.ack.<id>`) and a Mini App button; the Menu gets
"ข่าวล่าสุด", the Student's `[announcements] latest` newest (default 5, 1 to 10),
with "ดูทั้งหมด" to the Mini App list on the last Card. The Audience can also be a hand-picked list of student IDs (up to
2,000): paste them one per line or separated by commas or spaces, or search the
Students and press "เพิ่ม" (needs `kernel.student.view`). The list is one more
AND filter; on its own it is the whole Audience. The preview lists the IDs that
match nobody (or do not fit `[student] id_pattern`) and the Students without a
live Link; none of them is sent to. Anyone who may send (`announcements.send`)
sees that preview, so it reveals roster membership and Link status of the IDs
they paste, even without `kernel.student.view` (which only the search box
needs); the detail page shows the saved list to anyone with
`announcements.view`. IDs may also be separated by `;` and zero-width characters
from a spreadsheet paste are ignored. The list is saved with the announcement
(shown on its detail page) and the audit rows record its size, not the IDs.
Before rolling back to a build without hand-picked lists, cancel or pause every
scheduled hand-picked announcement: an older build ignores the list and would
send it to the remaining filters, possibly everyone.
The detail page shows acknowledged / in Audience and every
delivery; edits reach Telegram through `EditMessage`. LINE cannot edit a sent
message: tick "แจ้งผู้รับว่ามีการแก้ไข" when saving the edit of a sent
announcement and Job `announcements.notice` sends each recipient there (the
newest delivery per live Link, hand-picked ones included, those who already
acknowledged too: their acknowledgement stands) whose text changed one short
Push linking to the updated announcement, metered like any Push and keyed per
edit version and recipient, so a re-run notifies no one twice. The box needs a
changed text (an expiry-only edit sends nothing) and shows the recipients on
LINE and this month's Quota use beside it, warning when the notice would go
over (it still sends). Each edit with the box ticked notifies everyone again:
the keys are per version. A later save with the notice stops a run still
going; a retract or the expiry stops it too (a Push already past the check
still goes out). The detail page shows sent / recipients and failures, and
the audit log `announcement.notice` with the count. An expired announcement
cannot be resent ("ส่งซ้ำ" is hidden): extend its expiry first. Past its expiry date an announcement leaves the Menu and the Mini
App list, and a Student opening it from a Card (the Mini App deep link) or
pressing "รับทราบ" is told "ข่าวนี้ไม่เปิดให้ดูแล้ว" and nothing is recorded
(before #77 an expired one still took the acknowledgement); Staff still see it
with "หมดอายุแล้ว". "ส่งซ้ำ" is a new round,
and a retract deletes where the Channel can (Telegram, within 48 h). Edits and
deletions are paced (about 250 a minute per Channel, resuming after a rate
limit) and a final retract pass runs 16 minutes later; the detail page shows
"กำลังดำเนินการ" until they finish.
Mini App `/app`: LIFF endpoint and Telegram Web App URL = the origin (`/` forwards to `/app`); 1-hour `app_session` cookie; `/notice` is public.

Administrators also get `/admin/security` (Security Events, `kernel.security.view`)
and `/admin/audit` (the audit log with a before/after diff, `kernel.audit.view`);
each request to either is itself logged as a sensitive read. `/admin/staff` and
`/admin/roles` (`kernel.staff.manage`) create Staff with a temporary password
shown once, disable them (their sessions end), assign Roles, reset passwords,
clear lockouts and edit Roles; at least one active Staff member always keeps
`kernel.staff.manage`. A Role from `[roles]` can be edited or deleted there too,
and the edit sticks: a later `migrate` grants only Permissions new to `[roles]`
(see the link Permissions above). `/admin/texts` (`kernel.setting.edit`)
edits any catalog text per Locale (it must keep the default's `{name}`
placeholders) and bumps the privacy notice version, which asks every linked
Student to accept again on their next message. Edits take effect on the next
message: `setting` is read on every Inbound, with no cache.
`/admin/backups` (`kernel.backup.manage`) lists the nightly backups, downloads
one as a `.tar`, starts one now, and downloads all data as an Excel workbook
without secret columns; see [Backups](backups.md).
