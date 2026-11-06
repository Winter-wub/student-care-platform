# Security policy

harpseal handles students' personal data (names, student ids, emails,
chat accounts), so we take reports seriously.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through GitHub:
on <https://github.com/Winter-wub/student-care-platform/security>, choose
**Report a vulnerability** (private vulnerability reporting). Include what
is affected (crate and version, or commit), how to reproduce it and what an
attacker gains.

We acknowledge a report within 5 working days, keep you informed while we
fix it, and credit you in the release notes unless you prefer otherwise.
Please give us a reasonable time to release a fix before disclosing.

## Supported versions

Until 1.0, only the latest released version of each crate receives fixes.

## Scope

In scope: the crates under `crates/` and the example under `deployments/`
as shipped. A customer's own Deployment (its configuration, Modules and
hosting) is the customer's responsibility; report issues in it to them.
