# F0 reference flows — IDE governance panels (#104)

Collected 2026-09-24 before drawing `../../flows/ide-governed-run.mmd`. Evidence, not answers:
each borrowed idea is re-checked against the first-value path in `../../ide-governance-brief.md`.
Details marked *unverified* could not be confirmed from the opened source.

| # | Reference | Source | Entry → screens | Errors / empty / loading | Borrow | Drop |
|---|---|---|---|---|---|---|
| 1 | MLflow Tracking UI — runs list → run detail | https://mlflow.org/docs/latest/ml/tracking/ | runs table (1 row = 1 run) → run detail (params, metrics, artifacts) · 2 | not documented (*unverified*) | Row click opens a detail that *is* the receipt; list and detail side by side | Metric history as a time-series chart — our runs carry no time series |
| 2 | Sigstore Rekor search / transparency log | https://search.sigstore.dev · https://docs.sigstore.dev/logging/overview/ | search form → entry list · 1 (detail layout *unverified*) | *unverified* | "Tamper-evident, not tamper-proof": verdict computed from the log itself | Any verdict reduced to one coloured badge — we name *where* the chain breaks in text |
| 3 | Marquez (OpenLineage reference UI) — column lineage | https://github.com/MarquezProject/marquez (column lineage since 0.27.0) | search → lineage graph → dataset drill-down · 1 | not documented (*unverified*) | Selecting an output column filters/highlights only its source path | A full graph as the default — table first (exact list), diagram optional |
| 4 | Styra DAS bundle registry — policy packs | docs.styra.com/das/policies/bundles/bundle-registry (search snippet only; page fetch failed) | active bundle + deployment history + retention count (*unverified*) | *unverified* | Active pack, append-only change history and retention on one surface | Structure not copied — concept only |
| 5 | Snowflake differential-privacy admin | https://docs.snowflake.com/en/user-guide/diff-privacy/differential-privacy-admin-adjust | SQL only (no screen) | n/a | Data model: spent · remaining · window · scheduled reset | One-line reset without confirmation — ours is confirmed and audited |

Not collected within the source budget: GitHub Actions run history, OPA/Styra rendered screens,
OpenDP/Tumult accounting UIs.
