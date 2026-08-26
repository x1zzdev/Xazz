# Monitor view override

Overrides `../MASTER.md` only for the Monitor canvas view. Inherits
`workspace.md` in full, including its dark token column, no-glow rule,
and trust contract. Added manually for `0TO1` work inside an accepted master; the ui-ux-pro-max
persist command was **not** re-run, so `MASTER.md` is unchanged.

## Layout

- Occupies the compiler-canvas region only. Topbar, source rail, inspector, and result dock keep
  their workspace rules and are not restyled.
- Below 1280px the three panels stack in one column. From 1280px the region is two columns: the
  measured Burn panel occupies the left, and the two proposed panels stack in the right. This is
  a correction — an earlier draft gave Burn the full width, which pushed both proposed charts
  below the fold at the 1440x960 reference frame and left the privacy chart unreadable without
  scrolling. No panel scrolls its own chart.
- Panels are separated by borders and the 12/16px control-group rhythm, not by stacked shadows.

## Evidence-status expression

The single job of this view is to make three different evidence statuses distinguishable at a
glance. This is expressed structurally, never by colour alone:

- **Measured** (`Beta`, Burn): solid `--surface-1`, values in the tabular data family at full
  `--text-1` contrast.
- **Proposed** (`Research`/`Planned`, DP and resource): `--surface-2`, a 1px `--control-border`
  top rule, values at `--text-2`, and a permanent scope line above the chart.
- Proposed panels draw their bars as **unfilled 1px dashed outlines** rather than solid fills.
  A measured bar is filled; an unmeasured bar is visibly hollow. This carries the distinction
  pre-attentively without colour and without a hatch — an earlier draft specified a diagonal
  `repeating-linear-gradient` hatch, which is rejected because `pages/workspace.md`, inherited in
  full, forbids gradient fills in tool chrome.

## Colour

- Proposed panels are restricted to `--text-2`, `--text-3`, `--border`, and `--control-border`.
  They may not use `--brand`, `--success`, `--warning`, or `--danger`; a status colour on an
  unimplemented capability is the failure this view exists to prevent.
- The Burn panel may use `--info` for the validation-loss series and `--brand` for training loss.
  Series are also distinguished by label and by position in the table alternative.

## Anti-generic constraints for this view

Inherits the master's rejected recommendations (no cinematic treatment, glassmorphism, glow,
ambient blobs) and adds:

- No gauge, speedometer, donut, or radial meter. These read as live instrumentation and every
  quantity they would show is unmeasured.
- No sparkline, no area fill under a line, no time axis. There is no time series anywhere in the
  available contract; drawing one is fabrication.
- No count-up number animation.
- The Burn loss comparison is two labelled points, never a connected curve.
- No KPI card row across the top. The three panels are not equally credible and a uniform card row
  would assert that they are.

## Signature

The master's `.xzz` bracket-and-compiler-rail motif appears once, as the thin vertical rail that
joins the three panels down the left edge — the same rail that joins pipeline nodes in the graph,
so the Monitor view reads as the same program seen from a different angle.
