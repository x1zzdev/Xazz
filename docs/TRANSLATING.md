# Translation guide

The English [README](../README.md) and English docs are the source of truth. Translated READMEs are short entry pages, not line-by-line copies. Keep commands, paths, version claims, and links identical to the current source and verify them against the repository before merging.

## Priority

1. `ARCHITECTURE.md` — first: readers need the compiler/runtime boundary to understand examples and limitations.
2. `SECURITY_GUARDRAIL.md` — second: policy behavior and safety claims must be translated precisely, with rule IDs unchanged.
3. `ROADMAP.md` — third: plans change often, so translate only when someone can maintain the update cadence.

These are priorities, not claims that translated copies already exist. Until then, translated READMEs link directly to the English documents.

## Updating a translation

1. Compare the current English source with the source commit recorded in the translation's status note. If the source changed, review every command, link, supported-platform statement, and feature claim.
2. Update the translated text and its source commit/date in the same PR. If the English source changes but a translation cannot be reviewed, change its visible status note to **⚠ Translation may be outdated** and link the source change. Do not silently advance the source commit.
3. Check all relative links and run any copied commands against the current tree. Keep code identifiers, CLI flags, filenames, policy rule IDs, and error strings untranslated.
4. Request review from a fluent speaker of the target language and a maintainer familiar with the feature. Check meaning and technical accuracy separately; a language review alone does not verify runtime claims.

For new document translations, put the source path, commit, and date at the top. Link back to the English original. The translation status note is the stale marker; it is intentionally visible without requiring a badge service.
