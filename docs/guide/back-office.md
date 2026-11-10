# The back office

What Staff see under `/admin`, and the Permission each page needs. Back to
the [README](https://github.com/Winter-wub/student-care-platform#readme).

`/admin` is the term status page (#123): status cards for the last roster
import, announcements, the message quota and the last backup, plus any a
Module adds, each shown only to Staff holding its Permission. The
navigation groups the pages as **งานประจำ** (Students, the roster,
announcements, Module pages) and **ผู้ดูแลระบบ** (Security Events, the audit
log, Staff, Roles, texts, settings, usage, branding, backups). The
administration pages keep raw details (cron, table names, IPs, request ids,
Permission ids, config file names) behind a closed "ข้อมูลทางเทคนิค"
disclosure. See [deployment.md](deployment.md) for a Module's group and card.

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
lists them and offers `template.csv`: the headers and one fictional example row,
#134; a file that still holds that row, its code, name and email together, is
refused naming the row, checked before the rest so a code pattern that refuses
the example's code still names it, with the file's other problems kept in the
report), choose `full` (Students missing from the
file become left and lose their Links) or `upsert` (add and update only;
nobody becomes left). The form lists `upsert` first and preselects `[roster] default_mode`
(default `upsert`, #120), and a request that sends no mode uses it too. A
`full` preview's confirm button names how many Students will become left
("ยืนยัน: N คนจะพ้นสภาพ") in the danger style; when the preview also shows the
red warning, Staff must type that number first (#135: only the number), and a
confirm without it is refused and changes nothing. The warning gives the numbers
("48 จาก 50 คน (96%) จะพ้นสภาพ และถูกตัดการผูกบัญชีแชท 40 บัญชี", or, for a
file shorter than the roster, its rows against the active Students), and the
typed field names it and the leaving figure with `aria-describedby` (#134). That number is from the
preview (upload time); the left phase runs when the import's Job does, so a
Student added or edited by hand in between can move the actual count. A Deployment whose files
never list every Student (one file per cohort) sets `modes = ["upsert"]` and
`default_mode = "upsert"`: the form then offers only `upsert`, and a request
asking for `full` is refused with 400 and imports nothing (`modes` must not be
empty, hold only `full` / `upsert` and include `default_mode`; without a
`default_mode`, the form preselects `upsert` when listed, else the only mode; a bad value stops
the Deployment from building). The preview and the import page name the mode.
A file refused for its header row states it once and sets the columns found in
the file (each marked as matching, or unknown or repeated) beside the columns
expected (those every row must fill apart from those that may be empty, each
marked as present or missing), with the template and an upload form on the same
page (#134). A **blocking** problem refuses the
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
no value). Each warning names its Student (code and name) on the page and in a
last `report.csv` column, `นักศึกษา` (#134). **Since #134 a blank optional cell
raises no warning by default**: an optional column may be left empty. A
Deployment that lists `empty_attribute` in `[roster] blocking` still refuses the
file, one problem per column and sheet with the count and the first rows. A
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
characters is an `invalid_email`). A preview leads with what changes (#134):
new, updated and will become left as three large figures (leaving in the refused
colour when anyone leaves, with the chat Links it cuts), then unchanged /
returning / fields kept by manual edits / kept active because Staff set the
status / rows / active before on one line. Under them, collapsed, who changes:
student code, name and each changed field as old → new, labelled with the
`[roster]` labels (an enum value with its label); and the leaving list: code,
name and the live Links each loses. Both are computed once at upload and kept
in the FileStore, never in the import's row: the whole list as
`roster-diff/<import id>.jsonl`, downloaded as `changes.csv`
(`kernel.roster.view`, audit `roster.changes.download`), and the first 100
changes and 200 leavers, each value cut to 200 characters, as
`roster-diff/<import id>.head.json`, which only the preview page reads. A
cancelled or replaced preview's lists are deleted at once. When they are not
available (deleted after a year, below, or missing from a restored backup), the
preview keeps its figures and says so in one line, and `changes.csv` answers
410 with a Thai page. The
`roster.import` audit row keeps the preview's counts only, no Student. Confirm runs the Job
`kernel.roster.import` in 500-row chunks (the page polls its progress), and the
done page shows each preview number next to what was imported. One import at a
time; the file stays in the FileStore under `roster/<import id>/` (its parsed
rows under `roster-rows/v2/<import id>.jsonl`, its changes under
`roster-diff/<import id>.*`) for one year, then the per-minute tick deletes
them (audit `roster.file.prune`) and drops the Students the import's row names
(who left, and the Student of each warning); the import's page and report
stay, with their counts, rows and reasons.
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
The Audience is an explicit choice (#120): "ทุกคน", "เลือกกลุ่ม" (at least one
attribute value or Tag) or "เลือกรายคน" (at least one ID). None is
preselected on a new announcement, and empty filters no longer mean everyone:
a form without a choice, or with a choice and nothing in it, is refused in
Thai and nothing is saved. "ตรวจก่อนส่ง" saves a draft and opens a review step
(`/admin/announcements/{id}/review`) before anything is sent or scheduled: the
bubble, the Students and accounts per Channel (counted as the form counts), the
message quota in plain Thai ("จะใช้ข้อความ LINE ประมาณ N ข้อความ จากโควตาเดือนนี้ที่เหลือ
M"), and when it goes out; then "ยืนยันส่ง" or "กลับไปแก้ไข" (the draft stays).
A send to everyone confirms with "ส่งถึงทุกคน N คน" in the danger style, and the
server counts again: a confirm without that N, or with one that no longer
matches, is refused and sends nothing. "ส่งซ้ำ" to everyone asks the same way.
N is the count at the moment of confirming: a scheduled send to everyone
goes to everyone linked at `send_at`, so a roster import in between moves the
number. With "count unreachable accounts too", the button names both
("ส่งถึงทุกคน N คน (ติดต่อได้ M)"). Editing a scheduled announcement's
Audience holds it: the schedule is cancelled, the edit is saved on a draft, and
it goes out again only after the review step and its confirm (texts and
timing alone save as before). Past the remaining quota the review step says
what happens: on LINE Free, LINE refuses the rest and those accounts do not get
the announcement (announcements do not fall back to email; `notify` does); a
package that buys extra messages goes on and may cost more; any other Quota
only warns. The review step and its `reviewed` field only guard against an
accidental click for group and hand-picked sends; CSRF and the Permission are
the security gates, and the everyone recount and the typed roster number are
the server gates.
A confirm posted without the review step is sent to it. The code API
(`announcements::create_draft` / `confirm`) takes an `Audience` value, so
`Audience::everyone()` there stays an explicit choice.
Saved Audiences (#144, Module migration `0004_saved_audiences`, native and
Workers alike): under the count, "บันทึกกลุ่มผู้รับนี้" saves the Audience part
under a name (unique, up to 60 characters, at most 100 saved), checked as a
draft's Audience is (a choice made, values `[roster]` has, at most 2,000
IDs). It keeps the rules (the choice, attributes, Tags, hand-picked IDs,
"count unreachable accounts too"), never a list of Students, so the reach is
counted again on the editor and on the review step. "เลือกจากกลุ่มผู้รับที่บันทึกไว้"
at the top of the Audience part fills it from a saved one;
`/admin/announcements/audiences` lists them with a link that starts a new
announcement from one (`/admin/announcements/new?audience=<id>`, which says
which one filled the form, or that it is gone), rename and delete after a
confirm step (announcements that used it keep their own copy). Saving and
picking on the editor need HTMX (JavaScript): without it the save box stays
hidden and the picker does not load; the list page's link still works. Enter
in the name box saves the Audience, not the announcement. A row whose stored
rules do not read (broken JSON, or a version this build does not know) says
"อ่านกลุ่มนี้ไม่ได้ ลบแล้วบันทึกใหม่", is never read as everyone, and can only
be deleted. A saved value that
`[roster]` no longer has (an enum value or a whole attribute removed) is shown
with a warning, still ticked, and refused until unticked: it never drops out
silently, which could widen the Audience. Every page needs
`announcements.send`; saves, renames and deletes are audited
(`announcement.audience_save`, `_rename`, `_delete`; the audit row counts the
hand-picked IDs, never lists them).
The body (up to 2,000 characters, as before) takes a small format (#108):
`**bold**`, `*italic*`, lines starting `- ` (bullets) or a number from 1
to 99 and `. ` (numbered, shown with the number typed; `2567. …` is a
paragraph), `[text](https://…)` links and bare `https://…` URLs. Only
`https:` links that LINE and Telegram both accept count: a host of two or
more labels (each up to 63 bytes, the last not all digits, 253 bytes in
all), no user info, no quotes, brackets or spaces, valid `%XX`
escapes, one `#` at most, at most 1,000 bytes; parentheses inside a
`[text](…)` URL must balance. Anything else, `http:`, `javascript:` and
`data:` included, and any HTML, stays text as typed. Emphasis and links
stay on one line; lists do not nest. A link's text loses invisible
characters; one with nothing visible left (`[](https://…)`) shows the URL's
host. For a body over 2,000 characters the preview shows "ยาวเกิน 2,000
ตัวอักษร" instead (a request over 128 KB is refused with 413). The form's
toolbar (B, I, •, 1., H, 🔗, 🖼 and the five colours) inserts the syntax (without script the help line
shows it), and "ตัวอย่างในมินิแอป" shows the body exactly as the Mini App
does, refreshed as Staff type (`POST /admin/announcements/preview`). The Mini
App shows the format; a link opens outside the chat app
(`liff.openWindow({ external: true })` on LINE once LIFF is ready,
`Telegram.WebApp.openLink` on Telegram, else the link opens a new window
itself with `rel="noopener noreferrer"`). In chat
the Card body is plain text (markup removed, link text kept, list markers
as typed) and the Card adds a URL button for each of the body's first two
distinct links, labelled with the link text (cut to 40 characters,
invisible characters removed, the host when nothing is left); the
"ข่าวล่าสุด" list shows the plain text only. A body without the format looks
exactly as before; one that happens to contain `*word*`, a `- ` or `1. `
line, or a link in brackets now shows formatted.

**Headings and colours (#113).** A line starting `## ` is a heading,
`### ` a smaller one (the toolbar's H turns the caret's line into `## `,
then `### `, then back to text). `{red}text{/red}` colours its text; the
palette is fixed: `red`, `orange`, `green`, `blue` and `grey` (each a
swatch on the toolbar, each at least 4.5:1 on white; DESIGN.md "Text
palette"). Any other name (`{pink}`, a hex) stays text as typed. Colours
hold bold, italic and links, and never another colour. Colour adds
emphasis and never carries meaning alone: coloured text is also
semi-bold, so it still stands out for a reader who cannot tell the colour,
and links stay underlined, so blue text is not taken for a link. In chat a heading
is a plain line (its `##` left out) and colours are dropped; the Mini App
and the preview show both. A body that already held such a line or marker
now shows formatted.

**PDFs (#113).** Up to 3 PDFs (slots `pdf1` to `pdf3`), 10 MB each, one
new PDF per save (a Worker's memory): the form shows one file input and
says "แนบได้ครั้งละ 1 ไฟล์ บันทึกแล้วแนบไฟล์ถัดไปได้"; with 3 attached,
remove one and save first. PDFs are told by their bytes (`%PDF-` at the start and
`%%EOF` inside) and named `.pdf`: an HTML or any other file renamed `.pdf`
is refused. A PDF with a script, an embedded file, a launch action, a
form submission, a remote go-to, XFA or rich media is refused, as is one
with a string or stream left open to its end. This scan is a best-effort
filter (a compressed object stream can hide a key); the protection is the
sandboxed serving and the phone's own browser. A Thai hint names the limits and says never to attach personal
data: anyone with the link can open the file. The name uploaded is kept
(its folder, control and invisible characters removed, at most 120
characters). PDFs follow the images' rules: stored in `announcement_file`
(kind `pdf`) and the FileStore, served at `/announcements/files/<token>`
only while the announcement is sent and live, kept on their URL after a
sent announcement replaces or removes them until it is retracted or
expires, deleted at once before sending, and carried by backup and
restore. They are served as `application/pdf` with
`Content-Disposition: inline; filename*=UTF-8''…` (the Thai name), an
ASCII `filename` fallback, `nosniff` and the sandbox CSP. The Mini App
lists them after the body with their names and sizes and opens them
outside the chat app. The pushed Card gives the first PDF a URL button
labelled with its name, inside the 2-URL-button budget (the body's first
link, then the PDF), only when `public_url` is set; on LINE it opens in
the phone's browser (`docs/guide/channels.md`). Staff download them
from the announcement's page; the back office does not preview them.

**Images (#109).** The create and edit forms take up to 4 images per
announcement (slots 1 to 4): JPEG or PNG only, told by their bytes and
named as what they are (a PNG named `.jpg`, a JPEG without a frame header,
an SVG, a GIF or text is refused), at most 1 MB, 4096 pixels a side
and 16 megapixels each (so 4096 x 4096 is refused), never re-encoded or resized. The form warns, without
refusing, when the image on a chat Card (Thai or English) is over 1024
pixels a side:
LINE may not show a larger Flex hero. A Thai hint next
to the slots names the limits and asks Staff to shrink large images first
and never to put personal data in an image: each one is served on a public
URL. A refused file names the limit it broke; files chosen are lost when
the form comes back with an error (the browser cannot keep them), and the
form says so. A request over 15,204,352 bytes (14.5 MiB: 4 images of
1 MiB, one PDF of 10 MiB and 512 KiB for the rest) is refused (413) before
it is read.
`![คำอธิบายรูป](image:N)` in a body places image N there (the toolbar's 🖼
inserts it for the first image not placed yet); the Mini App shows each
image in place, lazily loaded, with its alt text, and the images not placed
after the body. On a draft or a sent announcement an edit may add, replace
(a new file in a slot) or remove ("ลบรูปนี้") images. Before it is sent, the
files removed or replaced are deleted from the FileStore. Once sent, Cards
already delivered show the old image, so it stays served on its own URL
(marked superseded, no longer on the pages or new Cards) until the
announcement is retracted or expires; it is kept after that, as nothing
purges announcements yet. On a sent announcement the form says so next to
"ลบรูปนี้": an image removed for personal data needs the announcement
retracted. An edit of a sent one edits its
Cards where the Channel can (Telegram) whenever the images change; the edit
notice follows the text alone. A retracted announcement keeps its images,
as it keeps its text: Staff still see them on its page.

The images are in the FileStore under `announcement-file/<id>/` and in the
table `announcement_file` (Module migration `0003_files`), so the backup
carries them. Each is served at `/announcements/files/<token>` (128 random
bits) only while its announcement is sent and not expired; a draft, a
retracted or expired one, a removed image and an unknown token all get the
same 404. Staff see every image at `/admin/announcements/<id>/files/<token>`
(`announcements.view`). The chat Card shows the first image the body places,
else image 1, as the LINE Flex hero or the Telegram link preview
(`docs/guide/channels.md`), only when `public_url` is set
(`docs/guide/self-hosting.md`).
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
(see the link Permissions above). `/admin/texts` (`kernel.texts.edit`) finds
and edits any catalog text per Locale (it must keep the default's `{name}`
placeholders). On the same page, Staff holding `kernel.setting.edit` bump the
privacy notice version, which asks every linked Student to accept again on
their next message; with `kernel.setting.edit` alone the page shows only that
version. Edits take effect on the next message: `setting` is read
on every Inbound, with no cache.

Texts have their own Permission (#165), so Student Affairs can edit the wording,
including a Module's texts through `harpseal::texts::set_override`, without
holding `kernel.setting.edit` (the privacy notice version, branding, the LINE
package, and every Module setting read-only). Before the split
`kernel.setting.edit` covered the Texts page, so it works like the link split:
`migrate` grants `kernel.texts.edit` once to every Role in the database that
holds `kernel.setting.edit` (migration 0019), and a `[roles]` entry that lists
`kernel.setting.edit` without `kernel.texts.edit` is seeded with both, with a
notice in the migrate report; list both to say what you mean. A Role that
lists `kernel.texts.edit` alone edits texts and no other setting. A Role whose
`kernel.setting.edit` an Administrator already took away is not given
`kernel.texts.edit` by this rule. To take texts away from a settings Role,
untick it on the Roles page. `harpseal::texts::SETTING_PERMISSION` is
deprecated: check `harpseal::texts::TEXTS_PERMISSION` before `set_override`.
`/admin/backups` (`kernel.backup.manage`) lists the nightly backups, downloads
one as a `.tar`, starts one now, and downloads all data as an Excel workbook
without secret columns; see [Backups](backups.md).

## Plain Thai for Staff (#125)

Staff read the back office in Thai, in the words of `GLOSSARY.md`. Ids,
catalog keys, config names and ISO dates stay in the "ข้อมูลทางเทคนิค"
disclosure.

- **Permissions** read as Thai sentences ("นำเข้ารายชื่อนักศึกษา"), grouped
  under the Module's name as Staff know it: "นักศึกษา เจ้าหน้าที่ และการตั้งค่า" for the Kernel, else
  the title of the Module's first back-office page. Give each Permission
  its sentence with `Permission::new(id, description).thai("…")`. A Module
  that has not yet done so shows its English `description`; see
  [deployment.md](deployment.md#permissions-in-thai).
- **Roles** keep the names stored in the database (`[roles]` is seeded
  once, so renaming a Role in the TOML would create a new one and leave
  Staff on the old one). Instead, label a Role in `[texts]`:

  ```toml
  [texts.th]
  "kernel.role.General Staff" = "เจ้าหน้าที่ทั่วไป"
  "kernel.role.Student Affairs Staff" = "เจ้าหน้าที่กิจการนักศึกษา"
  ```

  The Kernel already labels `Administrator` "ผู้ดูแลระบบ". The Staff and
  Roles pages show the label; forms still post the stored name, and the
  Roles page lists stored names in its technical details. No migration
  is needed. `build()` refuses a label for a Role that `[roles]` does not
  list (a typo); a Role created on the Roles page is named in Thai there.
- **Locales** read as "ไทย" and "อังกฤษ" (`kernel.locale.<tag>`). Name
  another Locale of the Deployment in `[texts]` the same way.
- **Dates** read `9 พ.ย. 2569 14:30`: the Buddhist year, Thai month
  abbreviations and a 24-hour clock in the Deployment's `utc_offset`.
  Months read `พ.ย. 2569`. Numbers have thousands separators (`15,000`).
  Module pages use the same helpers: `harpseal::ui::local_datetime(&s,
  ms)`, `thai_date`, `thai_month`, `thai_day` and `number`.
- **The audit log** reads each action as a sentence ("สร้างประกาศ") and
  each actor and target by what it is ("ระบบ", "ประกาศ"); the raw action,
  actor and target ids are in the disclosure. A Module labels its own
  actions with a `<module>.audit.<rest>` text (`fees.import` →
  `fees.audit.import`); an action without a label shows as written.
- **Texts** are listed under the page they show on, with a search (text,
  page or key) and a page filter. Each links by its Thai text; the key is
  on the edit page's technical details. The page lists only texts read
  through the saved edits (chat, Mini App, notices and emails). Role,
  Locale and audit labels, and the texts of the home, Backups and usage
  pages, are read from the catalog alone: change them in `[texts]`.
- **Roster errors** say "ทั้งไฟล์" or "ทั้งชีต" for a problem with no row.
  A header problem lists the headers the template expects, with a link to
  the template.

## Help where the work is (#127)

Staff learn the back office by doing its file-driven jobs, so the help sits
on the pages where they do them.

- **Upload help.** The roster's upload form has a panel beside it (under
  it on a phone): the accepted files (CSV or XLSX, at most 4 MB, a CSV
  saved as CSV UTF-8), each column `[roster]` maps with what it holds,
  whether it may be empty ("ต้องมีค่า" / "เว้นว่างได้") and an example,
  and the template link. Staff who may only look see the panel alone. A
  file refused for its header row shows the same columns under the list
  of expected headers. A Module's upload page gets the same panel from
  the columns it declares once
  ([deployment.md](deployment.md#help-beside-the-file-input-and-the-next-step-127)).
  Example values of the roster's own columns are texts
  (`kernel.upload.example_*`), fictional by default; attribute examples
  come from the attribute (the first allowed value, the bottom of its
  range).
- **The next step.** A finished roster import offers "แจ้งนักศึกษา": a
  new announcement where Staff choose the Audience themselves, since a
  roster file cannot name it exactly. A Module import that names its
  Students (the example's fees upload) prefills the Audience with exactly
  those student codes, once its Job has written the balances (before
  that, the page says the import is queued or failed). Nothing is saved or sent by opening it: the draft goes through the
  usual review. Staff without `announcements.send` see only the way back.
- **Empty lists** say what goes there and offer the one action that fills
  it, only to Staff holding its Permission: Students (import the roster),
  announcements (write one), Staff while only the first account exists
  (add Staff), roster imports, and the audit log and Security Events. A
  search or filter that matches nothing says so and offers to clear it.
- **First run.** While the Deployment is fresh, which means **no Student
  yet**, the home shows "เริ่มต้นใช้งาน" above the status ledger: set the
  branding, import the roster, add other Staff, send the first
  announcement. Once any Student exists (imported or added by hand) it is
  never shown, so established Deployments do not see it. Each step links
  to its page and ticks itself from data the Deployment already keeps:
  branding set in `[branding]` (any value other than the default) or
  saved on `/admin/branding`, a second Staff account, an announcement
  sending or sent. There is no table of its own and nothing to dismiss. A
  step the Staff member has no Permission for is not shown, nor the
  roster step where `[roster]` maps no file. If its query fails, the
  block says it could not be read, as a status card does.
- **The announcement editor** says what its previews will show before
  anything is typed, instead of an empty card.

## Branding

`/admin/branding` (`kernel.setting.edit`, held by the Administrator) sets the
institution's display name, the primary colour and the logo shown in the back
office, on the sign-in page and in the Mini App's header, without a deploy.
Until something is saved, the TOML values apply (`name`, `[branding] name`,
`primary_color`, `logo`). A value equal to the TOML's is no setting: it follows
the file. "คืนค่าตามไฟล์ตั้งค่า" deletes all three.

- The colour is `#RRGGBB`, and white text on it must reach a WCAG contrast of
  4.5:1. The page names the ratio when it refuses one.
- The logo is a PNG or JPEG told by its bytes (the file name does not count),
  at most 256 KB and 16 to 1024 px a side. It is served at
  `/branding/logo?v=<hash>` with its real type and `X-Content-Type-Options:
  nosniff`. The route is public because the sign-in page and the Mini App show
  it before any session.
- "ดูตัวอย่าง" draws the back-office header and the Mini App header with the
  submitted values and saves nothing. A logo chosen for the preview travels
  on in the form, so "บันทึก" after it needs no second upload.

Every save is one batch gated on its audit row (`branding.update`, or
`branding.reset`), with the values before and after. A form shown before
another save is refused ("conflict"): reload and try again. Saved values are
checked again when read (a restored or edited row falls back to the TOML
value), and the logo route serves only a key of the logo's own shape, with
`Content-Security-Policy: default-src 'none'`. The logo must be a whole PNG
(ending with its `IEND` chunk) or JPEG (ending with EOI). A page in another Worker
isolate may show the old look for up to 10 seconds.

**LINE menu images.** The same page replaces the images of the locked and
unlocked rich menus (PNG, 2500 x 843 or 2500 x 1686 px, at most 1 MB). The
check runs the adapter's rules (the #92 checks: type, size, dimensions) and
the Core's grid: a Menu with one row of tiles needs 2500 x 843 px.
"ตรวจรูปและดูตัวอย่าง" draws every Locale's tap areas over the image, so you
can see that each tile sits under its button. "ใช้รูปนี้กับทุกบัญชี" first
creates the new rich menus on LINE. If LINE refuses, nothing is saved and
every account keeps its menu. Then every account is moved to the new menu in
the background and the old one is retired, as after a Menu label save
([ADR-0011](../adr/0011-menu-relink-and-retire.md)). The page shows how many
re-link Jobs are still pending. "คืนรูปตั้งต้น" returns to the bundled or
`LINE_MENU_IMAGE_*` images the same way. Audit: `branding.menu_images` and
`branding.menu_images.reset`. A Deployment without LINE says so and offers
no upload: Telegram has no menu image.

Storage: Kernel settings (`setting`, Locale `''`, keys `kernel.branding.*`)
and content-addressed FileStore objects (`branding/<sha256>.png|jpg`,
`branding/menu/<sha256>.png`). A file no setting names any more is deleted
after the save. Backups carry both (see [Backups](backups.md)). From code:
`harpseal::branding::set_menu_images` and `reset_menu_images`.
