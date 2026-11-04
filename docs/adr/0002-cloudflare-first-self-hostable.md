---
status: accepted
date: 2026-11-04
---

# Cloudflare-first, but self-hostable

The first customer was quoted on Cloudflare (Workers, D1, R2, Queues/Cron), while Thai university IT departments often require data on their own servers. We build in TypeScript on Hono so the same code runs on Workers and on Node/Bun, use Drizzle with the SQLite dialect (D1 on Cloudflare, SQLite/libSQL file when self-hosted), store files through the S3 API (R2 or MinIO), and put scheduling and queues behind interfaces. We rejected Cloudflare-only (locks out self-hosting customers) and Node+Postgres in Docker (would invalidate the first quote and lose the cheap serverless tier).

## Consequences

- No Cloudflare-specific API may be called outside the adapter layer.
- The database stays within what SQLite supports; no Postgres-only features.
