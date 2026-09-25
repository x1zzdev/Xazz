# Data source connectors — DuckDB & PostgreSQL

`load("...")` accepts two database URI schemes besides files and columnar
sources. Both run a SQL query and hand the result to the normal Polars pipeline,
so every operator (`filter`, `groupBy`, `join`, …) works on the result. Both are
**eager** sources: the full result set is materialized before the pipeline runs.

```xzz
v cities = load("duckdb://:memory:?sql=SELECT 'seoul' AS region, 9700000 AS population") :: City
v rows   = load("postgres://user:pass@localhost:5432/mydb?sql=SELECT * FROM readings") :: Reading
```

Runnable demos: [`examples/duckdb/hello_duckdb.xzz`](../examples/duckdb/hello_duckdb.xzz)
and [`examples/duckdb/postgres_demo.xzz`](../examples/duckdb/postgres_demo.xzz).

## DuckDB

**URI forms**

- `duckdb://:memory:?sql=...` — in-memory database
- `duckdb://data.db?sql=...` — file-backed database

DuckDB is compiled with the `bundled` feature, so **no system install is
required** (self-contained). The SQL is the `?sql=` parameter; the `duckdb://`
scheme is stripped before the query runs.

**Type mapping.** Integer and floating-point columns become Polars numeric
columns (`DECIMAL` is read as `float`), strings stay strings, and `NULL` becomes
a null cell. Mixed/dynamic result types are resolved per column.

**Known limitation — no Parquet interchange.** In the bundled DuckDB
(`1.10505`), `COPY (...) TO '...parquet'` **segfaults**. The connector therefore
avoids the Parquet file-interchange path entirely and reads each row through
DuckDB's `ValueRef` API. Two consequences:

- In-memory (`:memory:`) is backed by an ephemeral on-disk database file, because
  `:memory:` combined with `COPY (...)` also crashed.
- Large results are read row-by-row and fully materialized in memory. Keep
  queries selective (`WHERE`/`LIMIT`), or use a file-backed database.

For large analytical reads, prefer a columnar file source
(`load("data.parquet")` / `load("data.arrow")`) or a file-backed DuckDB.

## PostgreSQL

**URI form:** `postgres://user:pass@host:port/db?sql=...`

The SQL is the `?sql=` (or `&sql=`) parameter; the rest of the URI is passed
through to the connection string, so additional parameters such as
`sslmode=disable` reach the driver unchanged.

**Type mapping.** `int`/`float`/`text`/`bool` columns are mapped to Polars
numeric/string/boolean columns; unrecognized types become nulls.

### Security: the connection is **unencrypted**

The connector always connects with `postgres`'s **`NoTls`** (plain TCP). It
cannot negotiate TLS, so:

- Credentials and query results travel in cleartext on the network.
- Treat it as **local-development only** — do not point it at a remote database
  across an untrusted network.
- A URI that *requires* TLS (`sslmode=require` / `verify-full`) is not supported.
  To encrypt a remote connection, terminate TLS in front of the driver: an SSH
  tunnel, `stunnel`, a cloud SQL proxy, or a reverse proxy, and connect to that
  local endpoint.
- Credentials are written in the `.xzz` source in plaintext. Do not commit real
  credentials; use a read-only, least-privilege role and keep the URI out of
  version control (e.g. pass it via an environment-specific file or `.gitignore`d
  module).

## See also

- [README — Features](../README.md#features)
- [docs/ROADMAP.md](ROADMAP.md) — Track A3 (connectors)
- [CHANGELOG.md](../CHANGELOG.md) — connector implementation notes
