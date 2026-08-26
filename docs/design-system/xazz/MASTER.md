# Xazz Design System

> Source of truth for the approved product UX prototype. Page files in
> `pages/` may override only the rules they name.

## 1. Product character

- **Product:** inspectable, typed, local-first pipeline workbench
- **Primary user:** Python data or ML developer
- **Experience:** immediate proof → explainable impact → explicit run → honest receipt
- **Visual voice:** precise, quiet, engineered, warm enough to feel approachable
- **Signature motif:** `.xzz` brackets joined by a thin compiler rail
- **Themes:** evidence-led light landing; calm-dark workspace
- **Density:** spacious marketing surfaces, compact but breathable tool surfaces

### Rejected generator recommendations

The initial `ui-ux-pro-max` search correctly suggested an operations landing and
status-aware interface, but its `cinematic`, glassmorphism, glow, ambient blob,
and dark-only recommendations conflict with approved requirements. They are
explicitly rejected. Xazz uses opaque surfaces, no decorative glow, and no
cyber-security theatre.

## 2. Foundations

### Color roles

| Token | Light | Dark | Use |
|---|---:|---:|---|
| `--canvas` | `#F4F6F1` | `#0D1210` | Page / workspace background |
| `--surface-1` | `#FFFFFF` | `#121916` | Primary surface |
| `--surface-2` | `#E9EEE8` | `#18211D` | Secondary surface |
| `--surface-3` | `#DDE5DF` | `#202B26` | Selected / raised surface |
| `--text-1` | `#111815` | `#F2F6F3` | Primary text |
| `--text-2` | `#43514A` | `#B8C5BE` | Secondary text |
| `--text-3` | `#66746D` | `#91A098` | Tertiary text; 12px default, 10px only for nonessential data micro-labels |
| `--border` | `#BCC8C0` | `#394A42` | Boundaries and dividers |
| `--control-border` | `#829089` | `#596C63` | Interactive and essential graphical boundaries |
| `--brand` | `#086C50` | `#5AD8AB` | Primary action / active trace |
| `--brand-strong` | `#064F3C` | `#8BE8C7` | Pressed / high-emphasis text |
| `--info` | `#155FA0` | `#83B9FF` | Informational evidence |
| `--warning` | `#8A5300` | `#F2BC65` | Needs attention / partial |
| `--danger` | `#A32929` | `#FF8A8A` | Failure / rejection |
| `--success` | `#126B4D` | `#64D6AA` | Verified success only |
| `--focus` | `#186FCE` | `#9AC7FF` | Keyboard focus |

Functional colors always pair with an icon or text label. `brand` is not a
synonym for success. Status axes use separate labels for maturity, process,
pipeline verdict, control, and integrity.

### Typography

- UI family: `ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`
- Code/data family: `ui-monospace, "Cascadia Code", "SFMono-Regular", Consolas, monospace`
- Figma mapping: use the available system UI font; Inter is an allowed
  representational fallback only when the runtime family is documented beside it.
- Type scale: `10 / 11 / 12 / 14 / 16 / 18 / 24 / 32 / 48 / 64`
- Body: 16px, 1.55 line height; dense tool body: 11–12px, 1.45
- Labels: sentence case, 11–14px, 600. Ten-pixel text is reserved for
  nonessential data micro-labels; avoid tiny uppercase tracking.
- Numeric data: tabular figures
- Desktop line measure: 60–72 characters; mobile: 35–55

### Space, radius, and depth

- Space scale: `4 / 8 / 12 / 16 / 24 / 32 / 48 / 64 / 96`
- Radius: `4` data cells, `6` tool controls, `10` panels, `14` landing proof
- Border: 1px default, 2px selection/focus
- Shadows: landing only, `0 12px 32px rgb(17 24 21 / 8%)`
- Workspace hierarchy comes from surface and border, not stacked card shadows.

### Motion

- Fast feedback: 120ms
- Standard state change: 180ms
- View transition: 240ms maximum
- Animate opacity and transform only.
- Motion communicates selection, progress, or cause-and-effect; never ambience.
- `prefers-reduced-motion: reduce` removes nonessential movement and shortens
  progress transitions.

## 3. Layout

### Landing

- 1440px frame, content max-width 1240px, 32px desktop gutters.
- Hero is a two-column proof composition, not an abstract illustration.
- Primary CTA appears once in the hero and once after the capability truth map.
- Mobile at 390px uses 20px gutters, a single column, and no tool chrome.

### Workspace

- 1440×960 reference frame.
- 56px top command bar.
- Three-part work area: 224px sources, flexible compiler canvas, 304px inspector.
- 232px result dock; allow the canvas to retain visual dominance.
- Use borders and aligned rails rather than wrapping every region in a card.
- Mobile IDE is intentionally out of scope.

## 4. Component rules

Figma source of truth:

- `04 · Components / Reusable component source`
- `Button` component set `40:27`
- `Status badge` component set `40:76`
- `Pipeline node` component set `40:125`
- local variables collection `29:145`
- local text styles under `Typography/*`

The editable Core frames are implementation-reference captures of the React
prototype. Reuse or extend the canonical sets above when normalizing those
frames for production; do not treat duplicated capture layers as library
instances.

### Buttons

- One primary action per state.
- Minimum 44px hit target; 40px visual height is allowed with extended hit area.
- Primary: brand fill, white/light-theme label or dark ink on dark-theme mint.
- Secondary: surface fill with visible border.
- Tertiary: text + optional Lucide stroke icon.
- Disabled uses semantic `disabled`, lower contrast, and explanatory helper text.
- Focus: 2px focus ring plus 2px offset.

### Status

Never compress different meanings into one green badge.

- **Maturity:** Available / Beta / Demo / Research / Planned
- **Process:** Starting / Running / Exited / Unavailable
- **Pipeline verdict:** Unknown / Partial / Failed / Succeeded
- **Control:** Not configured / Needs review / Approved / Rejected / Frozen
- **Integrity:** Not computed / Computed / Verified / Mismatch

Every status includes its axis name in accessible text. A process exit alone
must never produce a pipeline success state.

### Pipeline node

- Fixed title row, operation label, evidence line, and explicit state word.
- Variants: default, selected, running, warning, failed, success, stale.
- Selection highlights the connected code line and lineage rail.
- Runtime evidence is factual (`100-row sample`, `42 rows`, `−2 nulls`);
  unavailable metrics use `Not available`, never invented values.

### Data and receipts

- Tables keep labels, units, sampling scope, and sortable header state visible.
- Charts have a title, unit, series label, text summary, and table alternative.
- Receipt rows separate observed fields from unavailable future fields.
- Code hash is named `Code hash · computed` and marked `Not persisted`.

### Errors

Order content as: what happened → where → affected scope → safe next step.
Core actions are `Explain`, `Open code`, `Apply as draft`, and `Run full
pipeline again`. Partial retry, restore, cancel, and resume remain visibly
labelled Future until backend contracts exist.

## 5. Content rules

- Lead with outcome, then architecture.
- Prefer `Catch data errors before training starts` to stack-first claims.
- Name sample limits (`100 rows sampled`) and execution location (`Local`).
- Use `Live Check` only for side-effect-free sample validation.
- Use `Full Run` only after explicit preflight confirmation.
- Use `Process exited` when that is the only observed event.
- Never say `audited`, `sandboxed`, `policy passed`, or `budget safe` for the
  current implementation.

## 6. Accessibility and quality gates

- Normal text contrast ≥ 4.5:1; large text and essential UI boundaries ≥ 3:1.
- Decorative dividers may use the quieter `--border`; controls and essential
  graphical objects use `--control-border`.
- Logical headings and landmarks; include a skip link.
- Native buttons and controls; graph nodes also have a keyboard-selectable list.
- Focus is never removed without a replacement.
- Status is never color-only.
- Loading, empty, partial, success, error, and cancelled states are designed.
- No horizontal overflow at 390px landing or 1440px desktop.
- No external font, image, chart, or icon CDN.
- No placeholder copy, clipped text, or unsupported capability labelled Available.
