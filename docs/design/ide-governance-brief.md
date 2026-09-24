# IDE governance panels — brief, alternatives, behavior contract (#104)

- Mode: `0TO1` inside the accepted master (`../design-system/xazz/MASTER.md` is the visual-system
  constraint; `ui-ux-pro-max --persist` is **not** re-run). Profile `STRICT` for the design
  representation — inherited from `../design-evidence/ml-monitoring.json`: showing audit, policy
  or privacy state falsely would assert controls that do not exist. `journey_scope: touchpoint_only`.
- Issues: #107 run history · #108 audit chain · #109 policy packs · #110 DP ledger · #111 error
  boundary · #112 reduced motion · #113 run progress · #114 schema inference · #115 i18n · #116 lineage.
- References: `research/ide-governance/flow-refs.md` · Flow: `flows/ide-governed-run.mmd` ·
  Wireframe: `wireframes/ide-governed-run.html`.

## 1. Brief

| Field | Content |
|---|---|
| Actor | Python data/ML developer (P-001) and the contest reviewer watching the demo |
| Context | xazz-server already exposes `/runs`, `/security/audit/*`, `/security/policy*`, `/dp/budget*`, `/catalog`, `/schema`; the IDE calls five endpoints only (#104) |
| Job | Show that a pipeline run is governed: what ran, under which policy, what privacy it spent, and that the record was not altered |
| Observed problem | Server capabilities have no screen, so the security story cannot be demonstrated (#104, #108); runs cannot be reopened (#107); the run overlay cannot tell "slow" from "stuck" (#113) |
| User outcome | From the Monitor and the result dock the user can reopen a past run, see the tenant's live ε ledger, verify the audit chain and install/remove a policy pack — each value labelled with where it came from |
| Service outcome | The demo (#154) covers check → block → run → budget → audit → lineage without leaving the IDE |
| Success criteria | Every new panel renders loading, empty, error and success from the real server response; a tampered `audit.jsonl` renders `Integrity: Mismatch` and names the first bad record; nothing the server does not return is shown as measured |
| First value | Opening Monitor shows the live ε ledger and chain verdict without any run |
| Efficacy moment | After a Full Run the ledger's spent ε and the chain's record count both change, and History lists the new run id |
| Constraints | `state-contract.md` axes and vocabulary; `pages/monitor.md` (no gauges, donuts, sparklines, time axes, KPI card rows); master motion rules; local mode ignores tenant headers; bearer tokens never persisted |

## 2. Productization unit

Records to look up (runs, audit records, policy history) are **lists with a detail**, not
dashboards. Use cases to complete (install a pack, reset a budget, trace a column) are **one
explicit action with a confirmation where it destroys state**.

## 3. Structural alternatives

| | A. Governance section inside Monitor + History tab in the dock (selected) | B. New top-level "Governance" canvas view |
|---|---|---|
| Structure | Live ledger, audit chain and policy packs stack under the existing Monitor panels; runs live next to Preview/Receipt in the dock; lineage lives in the inspector | A sixth canvas view with its own three-column layout |
| For | The demo path already passes Monitor; runs sit beside the receipt they reconstruct; lineage sits beside the operation it explains | Clean separation, more room per panel |
| Against | Monitor scrolls further | Splits "what this run did" from "what the tenant allows"; one more view to learn; the receipt and its history drift apart |
| Decision | Selected — keeps each record next to the evidence it explains | Rejected |

Visual direction is inherited (`MASTER.md` calm-dark workspace); no new direction is introduced.

## 4. Behavior contract

Every async panel has exactly five states: `loading` (skeleton, no numbers), `offline`
(server unreachable — the fetch threw), `error` (server answered non-2xx — status and body shown
verbatim), `empty` (2xx with nothing to list), `ready`. A panel never shows a previous tenant's
data after the access settings change: changing access re-fetches and shows `loading` first.

| Panel | Trigger | Required behavior |
|---|---|---|
| Server access | edit tenant / token / actor | Values live in memory only; token is never written to storage; a note says local mode ignores them |
| Run history (#107) | open the History tab, finish a Full Run, press Refresh | `GET /runs`; newest first; max 50 (server limit, stated) |
| Run history | select a run | `GET /runs/{id}`; shows the process status (`Exited` / `Exit failed` — the server stores the exit status, not a pipeline verdict), rows, and the code hash with a plain "same code as the editor" / "differs" note (not an Integrity verdict: a different hash is an edit, not tampering); the audit records carrying that hash; rows are shown only if this browser session still holds that run's response — otherwise "xazz-server stores run metadata only" |
| Run history | `404` | "Run not found for this tenant" — cross-tenant and deleted runs are indistinguishable by design |
| Audit chain (#108) | open Monitor, Refresh, after a Full Run | `GET /security/audit/log` + `GET /security/audit/chain`; server verdict is authoritative |
| Audit chain | `intact: false` | `Integrity: Mismatch` in danger tone + the first record whose link or recomputed hash fails, computed in the browser and labelled as such |
| Audit chain | record has `prompt_hash` | marked "Inference audit" with prompt/response/model hashes; raw text is never stored or shown |
| Policy packs (#109) | Install | client JSON parse first; then `PUT /security/policy`; `400` shows the parser message; success re-fetches pack + history |
| Policy packs | Remove | confirmation dialog stating the tenant falls back to the global policy; `DELETE`; re-fetch |
| Policy packs | TTL save / clear | `PUT` / `DELETE /security/policy/history/ttl`; `ttl_secs: 0` means "keep forever" |
| Policy packs | policy load fails (`500`) | fail-closed banner: "Execution is denied until the pack loads" |
| DP ledger (#110) | open Monitor, after a Full Run | `GET /dp/budget`; linear track spent/total; `resets_at` countdown when a window is set, otherwise "No rolling window" |
| DP ledger | Reset | confirmation dialog noting the reset is attributed to the actor; `POST /dp/budget/reset`; shows the re-fetched value |
| DP ledger | in-flight reservation | shown when the server returns it; otherwise "Not available in this version (#123)" |
| Run progress (#113) | Full Run pending | elapsed seconds measured in the browser; lifecycle Sent → Executing on server → Result; epoch progress "Not available in this version" |
| Run progress | Stop waiting | aborts the browser request; copy says xazz-server keeps executing but a dropped request is currently not recorded in History, the audit chain or the ε ledger (observed 2026-09-24 against a local build) |
| Run progress | 5-minute timeout | same dropped-request state; primary action "Run again" |
| Schema (#114) | choose a CSV in a File Input node | `POST /schema`; preview name/type, fill the node schema and server `filePath`; > 50 MB refused before upload; offline → browser detection (UTF-8/EUC-KR, 20 rows) labelled as such |
| Lineage (#116) | Trace columns | `POST /catalog` with the current code; table default, diagram optional; selecting an output column highlights its sources; `400/422` shows the compiler message |
| Error boundary (#111) | a panel throws | that panel shows the error and "Try again"; topbar, other panels and dock stay usable; a corrupt saved DAG offers "Discard saved DAG" as an explicit choice |
| Reduced motion (#112) | `prefers-reduced-motion: reduce` | spinners, skeleton pulse, animated edges and transitions stop |
| Language (#115) | toggle EN/한국어 | every new string and the DAG editor follow the toggle; status-axis vocabulary stays English (`i18n.jsx` policy) |

Forbidden in all new panels: gauges or radial meters, count-up numbers, time axes, the words
"audited", "sandboxed", "budget safe", "policy passed".
