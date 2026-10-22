# Local security dependency patch

Source: crates.io `object_store` 0.13.2, Apache-2.0; LICENSE.txt and NOTICE.txt
are retained. All upstream source files are unchanged.

Only the normalized Cargo.toml dependency on quick-xml changes from 0.39.0 to
0.41. This removes RUSTSEC-2026-0194 and RUSTSEC-2026-0195. Polars 0.53 requires
the object_store 0.13 API; updating object_store to 0.14 is not a semver-compatible
substitution. Cargo.toml.orig records the untouched upstream manifest.

Remove this patch when an upstream release compatible with the selected Polars
version uses quick-xml >=0.41. The full dependency graph remains subject to
cargo-deny; vendoring does not exempt its dependencies from security checks.

The original package's .cargo_vcs_info.json records its upstream revision.
Validation must cover object_store's HTTP/XML tests and Xazz's workspace tests.
