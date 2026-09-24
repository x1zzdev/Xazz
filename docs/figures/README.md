# Figure sources

The eight diagrams in this directory are native SVG drawings. Their editable
sources are in [`src/`](src/), one source for each English and Korean output.
The SVG files next to this README are generated copies used by the Markdown
pages. No proprietary editor or Mermaid conversion is needed: edit the source
SVG as XML or in an SVG editor, then regenerate the published copies.

From the repository root:

```bash
python3 scripts/render_figures.py
python3 scripts/render_figures.py --check
```

The first command copies changed sources to their published paths. The second
checks that all published SVGs match and that every source parses as XML; it
exits nonzero if a published file is stale. Commit the source and output
together. For a new figure, create `src/<name>.svg`, run the first command,
then link `docs/figures/<name>.svg` from the relevant document.

Do not edit the published copy directly: regeneration will replace it. Keep
the English and Korean variants paired when changing shared content.
