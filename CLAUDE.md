# student-care-platform

A reusable core for student care and service systems that serve students over messaging channels. It is not a multi-tenant SaaS: each customer gets their own deployment, customised on top of this core. The first channels are LINE and Telegram. The first customer is มหาวิทยาลัยในไทย (via Fastwork). Keep channel-specific behaviour behind a channel layer, and keep customer-specific rules and content in configuration or extension points instead of the core.

## Agent skills

### Issue tracker

GitHub Issues in `Winter-wub/student-care-platform`, managed with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
