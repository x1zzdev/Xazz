# Xazz Documentation

Xazz is a typed pipeline language with a compiler, a Polars execution engine,
a deep-learning runtime, a policy guardrail, and a node-based Visual IDE. This
book collects the project's documentation in one browsable place.

If you are new here, start with the project [README](../README.md) for the
60-second tour, then come back for the deep dives.

## Where to go next

- **Understand the system** — [Architecture](ARCHITECTURE.md),
  [Workspace](WORKSPACE.md), [Glossary](GLOSSARY.md)
- **Run it** — [Docker](DOCKER.md), [GPU & ONNX backends](GPU_BACKENDS.md),
  [Data source connectors](CONNECTORS.md), [Check diagnostics](CHECK_DIAGNOSTICS.md)
- **Security & governance** — [Security guardrail](SECURITY_GUARDRAIL.md),
  [Security model](design/security-model.md)
- **Roadmap & process** — [Roadmap](ROADMAP.md), [Community](COMMUNITY.md),
  [Releasing](RELEASING.md), [Translating](TRANSLATING.md)
- **Design notes** — the [Design](design/ir.md) section

> This book is built from the Markdown files under `docs/`. Some pages link back
> to repository files (source, examples, `README.md`); those links are relative
> to the repository and are meant to be read on GitHub or in a checkout.

## Building this book

The site is built with [mdBook](https://rust-lang.github.io/mdBook/):

```bash
mdbook build       # output in ./book
mdbook serve       # live preview at http://localhost:3000
```

CI builds the book and verifies that internal links are not broken — see
[`.github/workflows/docs.yml`](../.github/workflows/docs.yml). The workflow
uploads the rendered site as the `xazz-docs` artifact on every run; publishing
to GitHub Pages is opt-in (enable Pages with the GitHub Actions source, then set
the repository variable `ENABLE_PAGES=true`).
