# Local security dependency patch

Source: crates.io `object_store` 0.13.2, Apache-2.0; LICENSE.txt and NOTICE.txt
are retained. All upstream source files are unchanged.

The only runtime change is the normalized Cargo.toml dependency on quick-xml,
from 0.39.0 to 0.41. A test target points to Xazz's loopback HTTP/XML regression
fixture in scripts/tests. This removes RUSTSEC-2026-0194 and RUSTSEC-2026-0195. Polars 0.53 requires
the object_store 0.13 API; updating object_store to 0.14 is not a semver-compatible
substitution. Cargo.toml.orig records the untouched upstream manifest.

Remove this patch when an upstream release compatible with the selected Polars
version uses quick-xml >=0.41. The full dependency graph remains subject to
cargo-deny; vendoring does not exempt its dependencies from security checks.

Validation must cover object_store's HTTP/XML tests and Xazz's workspace tests.
The upstream HTTP integration test silently skips without HTTP_URL; Xazz's
http_xml_security target instead requires a real local request and parsed
metadata, and verifies malformed XML rejection, on both Linux and Windows CI.

Upstream crates.io package SHA-256: `622acbc9100d3c10e2ee15804b0caa40e55c933d5aa53814cd520805b7958a49`.
