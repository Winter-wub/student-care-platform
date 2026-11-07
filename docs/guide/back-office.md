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
Staff who hold its Permission. Before the split `kernel.link.manage` covered
both: `migrate` grants `kernel.link.revoke` once to every Role in the database
that holds `kernel.link.manage`, and a `[roles]` entry that lists only
`kernel.link.manage` is granted both on every `migrate`. An entry that lists
`kernel.link.revoke` is taken as written, so list it alone for unlink without
Manual Link. A Role cannot be given Manual Link without unlink through
`[roles]`; make it on the Roles page. The attributes come from
`[roster.attributes.<key>]` in `deployment.toml`: `column`, `type` (`text`, `integer`, `number`, `date` or
`enum` with `values`), `label = { th = "..", en = ".." }`, `optional`,
`shown_in_list` and `order` (see `harpseal::RosterConfig`).

`/admin/roster` imports the roster (`kernel.roster.view` to see imports,
`kernel.roster.import` to upload). Upload a `.csv` (UTF-8, `,` or `;`; a
`.txt` is refused) or `.xlsx` whose headers match `[roster]` exactly (the page
lists them and offers `template.csv`), choose `full` (Students missing from the
file become left and lose their Links) or `upsert`. Any row error refuses the
whole file with a CSV report, including data in a column without a header.
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
`migrate` grants its TOML Permissions again. `/admin/texts` (`kernel.setting.edit`)
edits any catalog text per Locale (it must keep the default's `{name}`
placeholders) and bumps the privacy notice version, which asks every linked
Student to accept again on their next message. Edits take effect on the next
message: `setting` is read on every Inbound, with no cache.
