---
status: accepted
date: 2026-11-04
---

# Core repo plus one repo per customer

The product is sold as a customised system per customer, and we must be able to hand a customer their own source code while keeping ownership of the reusable part. We publish the Core from this repo as versioned packages; each customer gets a separate repo holding configuration, the Modules it switches on, and customer-specific code, pinned to a Core version. We rejected forks per customer (they drift and core updates become merges) and a single codebase with feature flags (other customers' code would ship with any handover).

## Consequences

- Every customer, including the first, has its own private repo from day one (this repo is public, see ADR-0005). During development the customer repo points at a local checkout of the Core (`file:`/`pnpm link`); before delivery it pins a published version.
- This repo keeps `deployments/example/`, a fictional customer used as the reference deployment and for end-to-end tests.
- Anything a customer needs that the Core does not expose is a Core change first, not a patch in the customer repo.
