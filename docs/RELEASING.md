# Releasing Xazz

> Release cadence, checklist, and the release-note template. See
> [GOVERNANCE.md](../GOVERNANCE.md) for who cuts releases and
> [CHANGELOG.md](../CHANGELOG.md) for the change log itself.
> Tracking issue: #158 (Track G).

## Cadence

- **Patch** (`vX.Y.Z`) — bug and security fixes, dependency/CI upkeep. Cut as
  needed; do not wait for a feature to land.
- **Minor** (`vX.Y.0`) — new user-facing features, new tracks/milestones. Aim
  for a steady rhythm (roughly every 4–8 weeks) rather than an ad-hoc date.
- **Major** (`vX.0.0`) — breaking language, CLI, or API changes. Requires a
  migration note in the release notes.
- Security fixes follow [SECURITY.md](../SECURITY.md) and may ship as an
  out-of-band patch; disclose only after the fixed release is published.

Releases are cut from `main`. There are no long-lived release branches except
when backporting a hotfix (see below).

## Checklist

Copy this into the release issue/PR and tick items as they complete.

- [ ] Scope frozen: all issues in the milestone are closed or explicitly moved.
- [ ] `CHANGELOG.md` `[Unreleased]` section is complete and grouped
      (Added / Changed / Fixed / Security / Performance).
- [ ] Workspace version bumped in `Cargo.toml` (`[workspace.package] version`).
- [ ] `cargo update` run and `Cargo.lock` committed if dependencies moved.
- [ ] Full local gates pass:
      `cargo fmt --all -- --check`,
      `cargo clippy --workspace --all-targets -- -D warnings`,
      `cargo test --workspace`,
      `cargo deny check --all-features`.
- [ ] Visual IDE checks pass (`visual-ide/`: `test:contract`, `test:contrast`,
      `test:e2e`) when the frontend changed.
- [ ] `CHANGELOG.md` gets a new dated section for the version; `[Unreleased]`
      is reset to empty.
- [ ] Release notes drafted from the template below.
- [ ] Docker/GHCR: the `docker` job in `.github/workflows/release.yml` will
      build `linux/amd64`+`linux/arm64` and push
      `ghcr.io/x1zzdev/xazz:<version>` (plus `latest` for stable). Confirm the
      tag is a stable `vX.Y.Z`, then run the clean-host smoke in
      [docs/DOCKER.md](DOCKER.md) and record the result (tag / OS / arch).
- [ ] Tag pushed: `git tag -a vX.Y.Z -m "vX.Y.Z"` then `git push origin vX.Y.Z`.
- [ ] GitHub Release published; verify all platform archives and
      `checksums.sha256` are attached.
- [ ] Announce: GitHub Discussions → Announcements, and link the release.

## Version bump

The workspace version is the single source of truth:

```toml
# Cargo.toml
[workspace.package]
version = "0.3.1"   # → next version
```

Crates inherit it via `version.workspace = true`. Bump once, in one commit,
before tagging.

## Cutting the release

```bash
# from a clean main that already contains the bump + CHANGELOG commit
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z
```

The tag triggers `.github/workflows/release.yml`, which builds the
Linux/Windows/macOS archives, generates `checksums.sha256`, publishes the
GitHub Release, and (for stable tags) pushes the multi-arch GHCR image.

## Hotfix / backport

1. Branch from the last release tag: `git checkout -b hotfix/vX.Y.Z vX.Y.(Z-1)`.
2. Apply the minimal fix and a regression test; update `CHANGELOG.md`.
3. Bump the patch version and follow the checklist, tagging the patch on the
   hotfix branch.
4. Merge the hotfix back into `main` so the fix is not lost.

## Release notes template

Copy into the GitHub Release body. Keep the user-visible summary short; link
issues/PRs for detail.

```markdown
## Highlights

- <one line per headline feature or fix, with #issue links>

## Added
- ...

## Changed
- ...

## Fixed
- ...

## Security
- <only after the fixed release is public; link the advisory>

## Upgrade notes
- <breaking changes and migration steps, or "None">

## Docker
```
docker pull ghcr.io/x1zzdev/xazz:X.Y.Z
docker run --rm -p 8005:8005 -v "$PWD/data:/data" ghcr.io/x1zzdev/xazz:X.Y.Z
```

**Full changelog:** https://github.com/x1zzdev/Xazz/compare/vPREV...vX.Y.Z
```

The `## Added/Changed/Fixed/Security` headings mirror the `CHANGELOG.md`
sections so the two stay in sync; the release body is the summarized view of
the same entries.

## After the release

- Reset `CHANGELOG.md` `[Unreleased]` to an empty section.
- Close the release issue and update `docs/ROADMAP.md` / README if a milestone
  completed.
- If a Track G community item shipped, tick it in
  [COMMUNITY.md](COMMUNITY.md).
