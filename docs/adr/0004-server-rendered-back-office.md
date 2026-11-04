---
status: superseded by ADR-0006
date: 2026-11-04
---

# Server-rendered back office with Hono JSX and HTMX

The back office must deploy to Cloudflare Workers and to self-hosted Node/Bun alongside the bot (ADR-0002), and Modules must be able to add their own pages. We render it on the server with Hono JSX and use HTMX for interactivity, in the same bundle as the API and webhooks. We rejected a React/Vite SPA (a second API surface and a plugin mechanism inside the SPA for Module pages) and Next.js (needs OpenNext on Workers and a separate bundle).

## Consequences

- A Module adds back-office pages by registering routes that return JSX; no client build step per Module.
- Highly interactive screens are limited to what HTMX plus small inline scripts can do.
