---
status: accepted
date: 2026-11-04
---

# The Core is open source under Apache-2.0, published to the public npm registry

Customers need a clear answer to "who owns the source code", and customer repos must be able to install the Core without a private registry token. We publish the Core packages (`@student-care/*`) to the public npm registry under Apache-2.0; each customer's own repo (configuration and customer-specific Modules) stays private and belongs to the customer. We rejected GitHub Packages (private, needs a token in every customer's CI) and AGPL (university legal teams are wary of it, and the business is selling customisation and operation, not the Core itself).

## Consequences

- Nothing customer-specific, no customer name, data format or credential, may land in this repo once it is public.
- Contracts state: customer owns their repo; the Core is Apache-2.0 with the copyright held by the author.
