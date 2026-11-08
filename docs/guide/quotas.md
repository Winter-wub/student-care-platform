# Quotas and the LINE package

How a Channel's monthly Quota is set, and how an Administrator picks the
LINE Official Account package in the back office (#115). Back to
[Building a Deployment](deployment.md).

A Quota (GLOSSARY) is the limit on metered Pushes per calendar month. It
warns Staff: a banner on every back-office page from 80 %, red at 100 %,
and one email per level per month to every active Administrator. It never
stops sending. The platform does that on its own: when LINE refuses a Push
because the account's monthly limit is used up, `notify` falls back to
email or defers ([notify.md](notify.md)), exactly as before.

## Where the limit comes from

| Channel | Limit |
|---|---|
| LINE | the package or Custom number saved on `/admin/usage`, else `[quotas.line] metered_per_month` |
| any other | `[quotas.<kind>] metered_per_month` (none, or 0: no Quota, usage is shown and never warns) |

So the TOML number is the **default** and also the **Custom value** until
someone saves a choice. A Deployment without `[quotas.line]` has no LINE
Quota until a package is chosen. The first option of the form, "use the
configured value", removes the saved choice (audited as `setting.reset`)
and the LINE limit follows `[quotas.line]` again.

## The package list

The list lives in code (`kernel/usage/package.rs`, `PACKAGES`), so a Core
release updates it when LINE changes its prices. LINE Thailand's figures,
read on **2026-11-08** from
<https://lineforbusiness.com/th/service/line-oa-features/broadcast-message>
(prices exclude 7 % VAT):

| Package | Monthly price | Messages included (the limit) | Extra messages |
|---|---|---|---|
| Free | none | 300 | cannot be bought |
| Basic | 1,280 THB | 15,000 | can be bought, 0.10 THB each |
| Pro | 1,780 THB | 35,000 | can be bought, 0.06 THB each |
| Custom | | a whole number from 1 to 100,000,000 | |

Cross-checked with LINE's notices of 2024-06-19
(<https://lineforbusiness.com/th/news/20240619_1>) and 2024-07-15
(<https://lineforbusiness.com/th/news/20240716_1>): Free is 300 messages
(it was 500) and Pro 35,000 at 1,780 THB, extra Pro messages 0.06 THB, from
2024-08-01. The price table on those notices is an image; the figures above
are the page's own text. The system does not read the plan from the LINE
API (out of scope), so the Administrator must choose the package that
matches what the account subscribed to. The limit is the included
messages; extra messages bought beyond them show as over the Quota.

## Choosing one

`/admin/usage` shows the package and its limit in the LINE card, with the
source and its date. Staff holding **`kernel.setting.edit`** also see the
form (a package list and a number for Custom). That is the Permission that
edits texts and the privacy notice version; the Administrator Role of the
example holds it, and any Role can be given it on Staff & Roles. Holding
`kernel.usage.view` alone shows the package, never the form, and a POST
without `kernel.setting.edit` is a 403 that writes nothing. The form
carries the CSRF token and a compare-and-set token of the value it
showed: when someone else saved first, the save is refused with the
conflict message. Choosing what is in force already changes nothing.

Custom takes digits only, `1` to `100000000`; anything else is refused
with a message and nothing is written. The number is ignored when a
package is chosen.

## Effect

The limit is read when a page renders or a Job runs; nothing is cached. A
change therefore applies to **the current month's remaining budget** at
once: the month so far is measured against the new limit. Switching from
Free (300) to Basic (15,000) with 1,000 Pushes already sent shows 1,000
of 15,000 and clears the banner; the usage page, the banner, the
"used X/Y" beside the Announcement send estimate, the
`kernel.usage.watch` emails and the tick that enqueues the watch all agree.
A change also starts the Quota emails over: the watch Job's record of the
levels already announced this month (`kernel.usage.notified.line`) is
deleted in the same batch as the change. So Over under Free, an upgrade to
Basic, then Warn and Over of Basic each email once. A Deployment with no
`[quotas]` starts the watch as soon as a package is saved (the tick reads
one `setting` row per minute, only while `[quotas]` is empty). The usage
page, the banner and the Quota job each read the usage rows and the
package in one query.

## Audit and backup

The choice is one row of the Kernel's `setting` table (key
`kernel.usage.line_package`, no Locale; `basic`, or `custom:2500`), so
there is no migration and every backup and restore carries it. Each change
writes the audit row `setting.update` (target `setting`/
`kernel.usage.line_package:`) with the value before and after, in the same
batch as the value. Before the first save the value before is the TOML
number as `custom:<n>` (`null` without one). A saved value that no longer
reads (an unknown package, a malformed number or a value that is not text,
for instance from a restored row of another version) falls back to the
TOML number and the page says so until Staff choose again.
