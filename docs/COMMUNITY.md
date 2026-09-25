# Community Roadmap

> Why this file exists: the second-round evaluation rewards *community expansion
> potential* — how the project is managed, how people join, and how knowledge is
> shared. This document is the plan behind that, and every item links to a
> GitHub issue. Track G is also reflected in [ROADMAP.md](ROADMAP.md#track-g--community-scale-docs-releases-playground)
> and the README roadmap table; this file is the plan behind it.

The plan runs in three steps, in order. Each step makes the next one cheaper:
docs lower the entry barrier, release/contribution rules keep the flow steady,
and only then does an install-free playground make sense.

---

## Step 1 — Lower the barrier (docs + discussion)

| Item | Issue | What it does |
| :--- | :--- | :--- |
| Open GitHub Discussions | #156 | A place for Q&A, ideas, and show-and-tell, so questions stop burying bug reports. Linked from README, CONTRIBUTING, and the issue chooser. |
| Multi-language docs | #157 | `README_ja` / `README_zh` plus a translation guide, so the project is readable outside Korea. Keeps the English source as the single source of truth. |

## Step 2 — Keep the flow steady (release + contribution)

| Item | Issue | What it does |
| :--- | :--- | :--- |
| Release & contribution guide | #158 | `docs/RELEASING.md` (cadence + checklist), a release-note template tied to CHANGELOG, and a contributor ladder in CONTRIBUTING. |
| Reflect the track in governance | #160 | Adds Track G to `docs/ROADMAP.md` and the README roadmap table, linking every item to its issue. |

## Step 3 — Let people try it (playground)

| Item | Issue | What it does |
| :--- | :--- | :--- |
| Web playground / hosted demo | #159 | Try `.xzz` without installing anything, with a policy-block example one click away. Scope is either a WASM build or a hosted `xazz-server` sandbox; abuse limits and isolation are part of the work. |

---

## How this maps to the evaluation

| Criterion | Where it shows up |
| :--- | :--- |
| Community expansion potential | This whole track; #156 and #158 in particular |
| Utilization | #157 (reach) and #159 (try-before-install) |
| Open-source appropriateness | #160 keeps the public roadmap honest and issue-linked |
| Presentation | #157 and #159 give the talk something concrete to show |

---

## Status

- [x] #156 Open GitHub Discussions
- [x] #157 Multi-language docs
- [x] #158 Release & contribution guide
- [x] #160 Reflect Track G in ROADMAP/README
- [x] #159 Web playground / hosted demo
