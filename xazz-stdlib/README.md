# xazz-stdlib — the Xazz standard library (issue #56, B2)

Reusable `.xzz` modules, importable with the `std/` prefix:

```xzz
import "std/common";
import "std/models";
import "std/math";
```

`std/...` modules are **embedded into the compiler** (via `include_str!`), so
they resolve everywhere with no filesystem setup — the `.xzz` files here are the
single source of truth and are versioned with the workspace.

## Modules

| Import | Provides | Notes |
|---|---|---|
| `std/common` | `TimeSeries`, `Measurement`, `AirQuality`, `Regression` | Common tabular schemas |
| `std/models` | `LinearRegressor`, `MLPSmall`, `MLPMedium`, `MLPDeep` | Reusable `model { ... }` architectures |
| `std/math` | `Stats` type, `Linear`, `SmallMLP` models | Math/statistics helpers |

Because Xazz has no function abstraction yet, the library provides reusable
**type** and **model** declarations; derived-value logic is expressed with the
built-in `withColumn`/aggregate operators at the call site. Richer `date`/
`string`/`statistics` *operators* are future work (they require language-level
operator additions).

## Usage

```xzz
import "std/models";

type Air = {
    observed_at:   string,
    district:      string,
    pm25:          Option<float>,
    temperature_c: float,
}

v dataset = load("data.csv") :: Air
    |> cast("pm25", "float")
    |> fillNull("pm25", strategy: "mean")
    |> select([temperature_c, pm25])

run dataset |> train(MLPSmall, target: "pm25", epochs: 5, lr: 0.01)
```

See [`examples/stdlib_import.xzz`](../examples/stdlib_import.xzz).

## Adding a module

1. Add `<name>.xzz` to this directory.
2. Register it in `xazz-compiler/src/modules.rs` → `STDLIB_MODULES`.
3. `import "std/<name>"` resolves it immediately.

> Field names must avoid reserved operator words (`mean`, `std`, `min`, `max`,
> `count`, `sum`, …) — the lexer tokenizes those as keywords. Use e.g. `avg` /
> `spread` instead.
