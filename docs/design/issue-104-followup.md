# Issue #104 follow-up (2026-09-25)

- Mode: PATCH, touchpoint only. Preserve the Monitor/Governance layout, dark tokens, tenant scope, keyboard controls, and measured versus proposed labels.
- Affected task: a tenant operator adjusts a DP window, audits resets and TTL changes, and checks a model response before release. Source: #241–#244 and the current server route contracts.
- Visible problem: four live server actions or histories have no IDE entry point. First value is a server-sourced result; efficacy is the updated value or audit row after an action.
- Code risk: STANDARD for the new server-backed behavior; #244 additionally handles potentially secret text in memory. No new storage or server permission is introduced.
- Anchor: `anchor-issue-104-0925` at `bcd2959c517409205370af2f005490965720f00d`; restore a file with `git checkout anchor-issue-104-0925 -- <path>`.
- Experiment budget: four of six remaining children have no backend or release dependency, so this pass can close at most 4/6 of the outstanding work in #104. #175 needs #165 and a prediction endpoint; #176 needs the v0.4.0 release.

| Issue | One hypothesis | Adopt when | Kill when |
|---|---|---|---|
| #241 | A local form around the existing DP panel exposes PUT/DELETE without changing the panel hierarchy | Contract check and focused browser path pass; 0 and override deletion show different sources | UI sends invalid input or shows a value not returned by the server |
| #242 | A small table under the ledger makes reset provenance visible | A reset immediately shows actor and prior spend in the focused browser path | A reset succeeds while the table silently stays stale |
| #243 | A paged list beside TTL controls makes retention changes auditable | Save/clear refresh list; next/previous use server offset | An old tenant's history is shown after access changes |
| #244 | An in-memory check form can show the runtime verdict without storing raw text | Blocked response, finding, hashes, audit index, and audit row appear in the browser path | Raw prompt/response enter browser storage or a blocked verdict looks safe |

Noise floor: zero contract failures and no false measured/safe state. Stop after one related suite and one focused rendered path per changed candidate; no repeated run on an unchanged diff.
