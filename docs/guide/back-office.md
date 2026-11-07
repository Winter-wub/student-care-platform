# The back office

What Staff see under `/admin`, and the Permission each page needs. Back to
the [README](https://github.com/Winter-wub/student-care-platform#readme).

`/admin/students` lists the roster (50 per page, HTMX search by name or code)
and opens a Student: fields, attributes, Links per Channel, Tags, Consent and
recent Security Events. Staff with `kernel.student.edit` edit fields (each
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
`kernel.link.revoke` is granted both on **every** `migrate` (on Cloudflare, every
cold start) and the migrate report prints a notice for it; update the entry to
say what you mean. An entry that lists `kernel.link.revoke` is taken as
written, so list it alone for unlink without Manual Link.

`[roles]` only ever adds Permissions. Narrowing an existing Role, say from
`kernel.link.manage` to `kernel.link.revoke`, means removing the Permission on
the Roles page **and** in `[roles]`; changing the TOML alone leaves it granted.
A Role that may Manual Link but not unlink must not be listed in `[roles]` (the
legacy rule would give it unlink back): create it on the Roles page.

The attributes come from `[roster.attributes.<key>]` in
`deployment.toml`: `column`, `type` (`text`, `integer`, `number`, `date` or
`enum` with `values`), `label = { th = "..", en = ".." }`, `optional`,
`shown_in_list`, `student_visible` (default `false`: shown to the Student
on a Module's Mini App page only when `true`) and `order` (see
`harpseal::RosterConfig`).

`/admin/roster` imports the roster (`kernel.roster.view` to see imports,
`kernel.roster.import` to upload). Upload a `.csv` (UTF-8, `,` or `;`; a
`.txt` is refused) or `.xlsx` whose headers match `[roster]` exactly (the page
lists them and offers `template.csv`), choose `full` (Students missing from the
file become left and lose their Links) or `upsert`. Any row error refuses the
whole file with a CSV report, including data in a column without a header.
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
characters is refused). Otherwise a preview shows new / updated / unchanged /
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
"ข่าวล่าสุด". The Audience can also be a hand-picked list of student IDs (up to
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
delivery; edits reach Telegram through `EditMessage`, "ส่งซ้ำ" is a new round,
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
`kernel.staff.manage`. A Role from `[roles]` can be edited there too, but the next
`migrate` grants its TOML Permissions again (see the link Permissions above for the
one that is implied). `/admin/texts` (`kernel.setting.edit`)
edits any catalog text per Locale (it must keep the default's `{name}`
placeholders) and bumps the privacy notice version, which asks every linked
Student to accept again on their next message. Edits take effect on the next
message: `setting` is read on every Inbound, with no cache.
`/admin/backups` (`kernel.backup.manage`) lists the nightly backups, downloads
one as a `.tar`, starts one now, and downloads all data as an Excel workbook
without secret columns; see [Backups](backups.md).
