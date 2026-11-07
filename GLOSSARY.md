# Student Care Platform

A reusable core for student care and service systems that talk to students over messaging channels. Each customer runs its own deployment built on this core. Terms are in English to match the code; the Thai equivalent follows each term for talking with customers.

## Language

### Shape of the product

**Core** (คอร์):
The reusable code this repo publishes. Made of the Kernel and Modules.
_Avoid_: platform, framework, base

**Kernel** (เคอร์เนล):
The part of the Core present in every Deployment: roster, linking, channels, menu, staff access, announcements, FAQ, inbox, audit, import/export, metering, backup.

**Module** (โมดูล):
An optional part of the Core that a Deployment switches on, such as exam results, fees or requests.
_Avoid_: feature, plugin, extension

**Deployment** (ระบบของลูกค้า):
One customer's running system: the Core plus that customer's configuration and customer-specific code.
_Avoid_: tenant, instance, site

### People

**Student** (นักศึกษา):
A person on the roster, identified by student ID. The only kind of person who talks to the bot. A Student is active while on the roster and left once a `full` roster import no longer lists them (a Deployment may allow `upsert` only, which never marks anyone left); a left Student keeps their records but loses every Link.
_Avoid_: user, member, end user

**Staff** (เจ้าหน้าที่):
A person who signs in to the back office with a system account.
_Avoid_: admin (that is a Role), operator, officer

**Role** (บทบาท):
A named set of Permissions that a Deployment's administrator defines and assigns to Staff.

**Permission** (สิทธิ์):
One specific thing the Core lets Staff do, such as adjusting a fee or exporting data. Defined by the Core, grouped into Roles by each Deployment.
_Avoid_: right, capability, scope

### Identity

**Channel** (ช่องทาง):
A messaging platform a Student can talk to the system through, such as LINE or Telegram.
_Avoid_: platform, provider, integration

**Channel Account** (บัญชีแชท):
A Student-side account on one Channel, such as a LINE user ID or a Telegram user ID.
_Avoid_: LINE account, chat user, profile

**Link** (การผูกบัญชี):
The tie between a Channel Account and a Student, created after Verification. A Student has at most one Link per Channel; a Channel Account links to at most one Student. A Link is active, unreachable (the Student blocked the bot; it recovers by itself) or revoked (by Staff or the Student).
_Avoid_: binding, registration, connection

**Audience** (กลุ่มผู้รับ):
A set of Students selected by attribute (year, programme, Locale, link status), Tag or a hand-picked list of student IDs; the filters combine with AND. Announcements and reminders are sent to an Audience.
_Avoid_: target group, segment, recipients

**Tag** (ป้าย):
A label on a Student so an Audience can select on it. A Tag comes from the roster file (replaced on every import) or is attached by Staff by hand (kept across imports).

**Quota** (โควตา):
The limit per period on one metered thing, such as LINE Push messages per month. Reaching it warns Staff; it never stops sending by itself.

**Verification** (การยืนยันตัวตน):
Proving that the person behind a Channel Account is a given Student, using the Deployment's Verification Method.
_Avoid_: authentication, login, registration

**Verification Method** (วิธียืนยันตัวตน):
A swappable way of doing Verification, such as email OTP. Chosen per Deployment.

**Consent** (ความยินยอม):
The record that a Student accepted a given version of the Deployment's privacy notice, when, and through which Channel Account. The notice is shown before Verification; the acceptance is recorded once Verification identifies the Student, and counts for every Channel. Asked again when the notice version changes.

**Security Event** (เหตุการณ์ความปลอดภัย):
A record of a suspicious act around Verification or data access, such as an OTP lockout or a request to see another Student's data, shown to Staff. It also records an Inbound event the system failed to handle, so a lost message is never silent.

**Manual Link** (ผูกบัญชีโดยเจ้าหน้าที่):
A Link that Staff creates directly, after checking the Student's identity themselves. Always available regardless of the Verification Method. Removing a Link (Staff unlink) is a separate Permission, so a Role can unlink without being able to Manual Link.

**Locale** (ภาษา):
The language a Student or a piece of content is in. Every Deployment has a default Locale that content falls back to.

### Messaging

**Channel Capability** (ความสามารถของช่องทาง):
Something a Channel can do beyond the required minimum, such as editing a sent message, sending a file to a Student, or opening a Mini App. The required minimum is receiving text and files, and sending Text, Card, Card List and Choices. A Channel adapter declares its Capabilities; the Core uses them when present.

**Reply** (ตอบกลับ):
An outgoing message sent in the context of a message the Student just sent, while the Channel still allows it. Free on LINE.
_Avoid_: response, answer

**Push** (ส่งถึงนักศึกษา):
An outgoing message the system starts on its own, such as an announcement or a reminder. Metered on LINE.
_Avoid_: notification, broadcast, multicast

**Outbound Message** (ข้อความขาออก):
The record of any message sent to a Channel Account, with whether it was metered on that Channel. Usage figures come from these records. A metered unit is one Push request to one recipient, however many messages it carries (LINE puts up to 5 in one request), so usage counts Push requests, not messages. A message the Channel has to spread over several Push requests (a very long text, a long Card List) costs one unit for each of them. Each record keeps its place in its batch and the units counted for it, so sending the same batch again after a failure never counts a Push request twice.

**Card** (การ์ด):
A structured message with a title, body and optional buttons. A **Card List** (ชุดการ์ด) is several Cards shown together.
_Avoid_: Flex, bubble, carousel, template

**Choices** (ตัวเลือก):
A short set of buttons offered with a message, each carrying an Action.
_Avoid_: quick reply, inline keyboard

**Menu** (เมนู):
The persistent set of entry points a Student sees in the chat. Set per Channel Account; locked until the Student has a Link. Identified by a key: the Locale it serves, or `locked`. Each back-office save of one of its labels makes a new revision, which every linked Student is moved to before the previous one is retired.
_Avoid_: rich menu, command list

**Action** (การกระทำจากปุ่ม):
What pressing a button means, stored on the server and referenced by a short id. Idempotent, and may expire.
_Avoid_: postback, callback

**Inbound Message** (ข้อความขาเข้า):
A text or file a Student sent outside any Action, kept by the Core so Staff tooling can read it later. The first slice stores it and answers with the Menu.

**Attachment** (ไฟล์แนบ):
A file a Student sent, copied into the Deployment's own file storage as soon as it arrives.

**Mini App** (มินิแอป):
A web page opened inside the chat app for what chat does poorly, such as long forms or long tables. Chat is always the primary flow. LIFF on LINE, Web App on Telegram.
_Avoid_: LIFF, web app, webview

**App Session** (เซสชันมินิแอป):
The identity of a Mini App page view, resolved from the Channel's signed token to a Channel Account and, through its Link, to a Student. Students have no separate login. It lasts one launch at most: each time the Mini App is opened from the chat the platform is asked again who is there, and the Student can sign out.

**Announcement** (ข่าว/ประกาศ):
A message Staff write once, in one or more Locales, and send to an Audience; it can expire, be edited, re-sent or retracted, and may ask each Student to acknowledge it. Once expired, Students can no longer open or acknowledge it; Staff still see it. Editing a sent one may notify its recipients where the Channel cannot edit the message (an Edit notice).

**Edit notice** (แจ้งการแก้ไข):
One short Push per recipient of an edited Announcement, on a Channel that cannot edit a sent message (LINE), linking to the updated Announcement. Staff choose it when they save an edit that changes a text; only recipients whose text changed get it, including those who already acknowledged (their Acknowledgement stands). Each edit notifies again; metered like any Push.

**Acknowledgement** (การรับทราบ):
A Student's one-time confirmation that they have read an Announcement, counted per Student regardless of how many Channels received it.

### Staff inbox

**Handoff** (ส่งต่อเจ้าหน้าที่):
The moment the bot hands a Student's message to Staff by opening a Conversation.
_Avoid_: escalation, ticket, forward

**Conversation** (เรื่องในกล่องข้อความ):
One exchange between a Student and Staff in the inbox, with a status of open, answered or closed.
_Avoid_: ticket, thread, chat

**Queue** (กล่องของหน่วยงาน):
A named group of Staff that Conversations are routed to, such as student affairs or finance. Defined per Deployment.
_Avoid_: department, team, inbox

**Office Hours** (เวลาทำการ):
The times Staff are expected to answer. Conversations opened outside them are marked as off-hours.

### Operations

**Job** (งาน):
A unit of deferred or scheduled work stored in the database and run by the Kernel when due. Large sends are split into many Jobs.
_Avoid_: cron, task, queue message

**Backup** (ชุดสำรองข้อมูล):
A copy of a Deployment's database and files, in backup format 1, that `harpseal restore` loads into an empty Deployment. A Job takes one every night into the Deployment's own file storage and keeps them for a set number of days; `harpseal backup` takes one by hand. "Download all data" (an Excel workbook without secrets) is a data export, not a Backup: it cannot be restored.
_Avoid_: dump, snapshot, export

**Fake Channel** (ช่องทางจำลอง):
An in-memory Channel adapter used in tests and demos; it must pass the same contract tests as real Channels.
