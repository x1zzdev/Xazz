# Governance

This document describes how the Xazz project is organized, how decisions are
made, and how contributions flow into releases. It is intentionally lightweight
and is expected to evolve as the contributor base grows.

## Principles

1. **Honest state contracts.** We do not present unimplemented capability as
   implemented. Status is marked `Stable`, `Preview`, `Planned`, or
   `Experimental` (see the feature table in `README.md` and
   `docs/design/state-contract.md`). A `#[ignore]`-gated test is *not* a shipped
   feature; it is documented as hardware-gated.
2. **Fail closed on safety.** Security and policy behavior defaults to blocking
   (for example, cyclic module imports and policy violations fail closed).
3. **Single source of truth.** The roadmap lives in `docs/ROADMAP.md`; the
   changelog in `CHANGELOG.md`; the dependency policy in `deny.toml`; the
   security process in `SECURITY.md`.
4. **Everything on `main` is green.** `main` must always pass `fmt`, `clippy
   -D warnings`, the full test suite, the frontend contract/contrast tests, and
   the dependency/license policy check.

## Roles

| Role | Responsibilities |
|------|------------------|
| **Maintainer** | Merge rights, release tagging, security triage, roadmap maintenance, final decisions. |
| **Contributor** | Issues, pull requests, reviews, documentation, examples, reproductions. |
| **User** | Bug reports, feature requests, discussions, feedback on usability. |

The current maintainer is the project author (`@x1zzdev`). Additional maintainers
are added from sustained, high-quality contributors.

## Decision making

- **Lazy consensus:** most changes proceed if no blocking objection is raised on
  the issue or pull request within a reasonable review window.
- **Design changes** (language syntax, IR shape, public API, breaking behavior)
  should start as a GitHub issue describing the problem and acceptance criteria,
  and must update `docs/ROADMAP.md`.
- **Disagreement:** the maintainer makes the final call and records the rationale
  in the issue or changelog.

## Contribution workflow

1. Open or claim an issue. Bugs use the bug template; features describe the
   problem and expected behavior.
2. Branch, implement with tests, and keep commits in
   [Conventional Commits](https://www.conventionalcommits.org/) style
   (`feat:`, `fix:`, `docs:`, `chore:`, ...).
3. Open a pull request. The PR template checklist must be satisfied:
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace`
   - `cargo deny check` (licenses, bans, sources, advisories)
   - Architecture constraints from `CONTRIBUTING.md` (the `xazz` CLI never links
     Polars/Tokio; `xazz-compiler` is Polars-free).
4. CI must pass on Linux, Windows, and macOS. A maintainer reviews and merges.

## Release process

- Versioning follows [Semantic Versioning](https://semver.org/).
- Releases are cut by tagging `vMAJOR.MINOR.PATCH`. The tag triggers
  `.github/workflows/release.yml`, which builds Linux/Windows/macOS archives,
  generates `checksums.sha256`, and publishes a GitHub Release.
- Every release updates `CHANGELOG.md` (Keep a Changelog format).
- Release artifacts are signed by checksum; the bundled Visual IDE is included
  under `web/`.
- The cadence, the pre-release checklist, and the release-note template live in
  [docs/RELEASING.md](docs/RELEASING.md).

## Quality & security management

- CI gates: `fmt`, `clippy -D warnings`, `test`, frontend `test:contract` /
  `test:contrast`, `cargo deny` (license/dependency policy).
- Dependencies are kept current by Dependabot (`.github/dependabot.yml`).
- Vulnerabilities are handled per `SECURITY.md`; dependency advisories are
  tracked by `cargo deny check advisories`.
- License compatibility is enforced by `deny.toml`; weak-copyleft licenses are
  scoped to explicit per-crate exceptions rather than globally allowed.

## Roadmap & issue tracking

- `docs/ROADMAP.md` is the single source of truth, organized by Track A–F.
- Each roadmap item maps to a GitHub issue; each issue carries a track label
  (`scale:*`, `genai:*`).
- The public status surface is the Phase table in `README.md`.

## Amendments

This document changes by pull request. Substantive changes are announced in the
project changelog.