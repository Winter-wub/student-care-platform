# The back office

What Staff see under `/admin`, and the Permission each page needs. Back to
the [README](https://github.com/Winter-wub/student-care-platform#readme).

`/admin/students` lists the roster (50 per page, HTMX search by name or code)
and opens a Student: fields, attributes, Links per Channel, Tags, Consent and
recent Security Events. Staff with `kernel.student.edit` edit fields (each
edited field is kept by roster imports until "ใช้ค่าจากทะเบียน"), manual Tags and
the status. Staff with `kernel.link.manage` create Manual Links and revoke
Links. The attributes come from `[roster.attributes.<key>]` in
`deployment.toml`: `column`, `type` (`text`, `integer`, `number`, `date` or
`enum` with `values`), `label = { th = "..", en = ".." }`, `optional`,
`shown_in_list` and `order` (see `harpseal::RosterConfig`).

`/admin/roster` imports the roster (`kernel.roster.view` to see imports,
`kernel.roster.import` to upload). Upload a CSV (UTF-8, `,` or `;`) or XLSX
whose headers match `[roster]` exactly (the page lists them and offers
`template.csv`), choose `full` (Students missing from the file become left and
lose their Links) or `upsert`. Any row error refuses the whole file with a CSV
report; otherwise a preview shows new / updated / unchanged / will become left /
returning / kept by manual edits, and Confirm runs the Job
`kernel.roster.import` in 500-row chunks (the page polls its progress). One
import at a time; the file stays in the FileStore under `roster/<import id>/`.
Try it with `deployments/example/data/roster.csv` (or `.xlsx`).

`/admin/announcements` (`harpseal::modules::announcements::Announcements`;
`announcements.view`, `.send`, `.retract`) creates an announcement (Thai text,
English optional), picks an Audience (roster attributes, any of their values,
AND Tags, AND Link status) with a live count and the metered Push estimate, and
sends it now or at a set time: Job `announcements.deliver` snapshots one
delivery row per live Link and starts one `announcements.deliver.channel` per
multicast group of 500 (LINE) or 25 accounts (Telegram, paced). The Card has
"รับทราบ" (`announcements.ack.<id>`) and a Mini App button; the Menu gets
"ข่าวล่าสุด". The detail page shows acknowledged / in Audience and every
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
