# Pull Request

## Summary

<!-- What does this PR change, and why? Link the issue: "Closes #123". -->

Closes #

## Type of change

- [ ] Bug fix (`fix:`)
- [ ] New feature (`feat:`)
- [ ] Documentation (`docs:`)
- [ ] Refactor / performance (`refactor:` / `perf:`)
- [ ] Chore / CI / build (`chore:`)

## Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo deny --workspace --all-features --locked check` passes (licenses · bans · sources · advisories)
- [ ] Architecture constraints respected (`xazz` CLI has no Polars/Tokio;
      `xazz-compiler` is Polars-free — see `CONTRIBUTING.md`)
- [ ] New behavior is covered by tests
- [ ] `CHANGELOG.md` and/or `docs/ROADMAP.md` updated when the change is
      user-visible or roadmap-relevant
- [ ] No unimplemented capability is presented as implemented (state contract)

## Notes for reviewers

<!-- For optional compute/backend/dependency changes: list compile checks separately
from hardware results (source SHA, features, toolchain, driver/runtime, exact executed
test count, CPU fallback rejection, evidence link). State untested providers. Update
#62/#63 or the relevant tracking issue when new hardware evidence changes its status.
Default workspace tests alone are not GPU validation. -->

<!-- Screenshots, benchmark numbers, migration notes, or anything the reviewer
should verify. -->
