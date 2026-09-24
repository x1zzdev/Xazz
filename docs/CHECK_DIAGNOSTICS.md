# `xazz check` diagnostic gallery

These four snippets fail before any CSV is opened. Save each as the indicated
`.xzz` file and run `XAZZ_LANG=en xazz check <file>`. The commands below were
reproduced with the current CLI; each exits 1 with one error.

**Location limitation:** The offending expression is on source line 3 in each
snippet, but `xazz check --json` currently reports `"line": 0, "col": 0` for
these semantic errors. The plain-text output does not show a location. The
`0:0` below is the actual reported value, **not** a valid source position.

| Case | Offending source line | Reported `line:col` |
|---|---:|---:|
| Did-you-mean | 3 | `0:0` |
| Undeclared column | 3 | `0:0` |
| Invalid cast type | 3 | `0:0` |
| Non-nullable `fillNull` | 3 | `0:0` |

## Did-you-mean: mistyped column

`typo.xzz`:

```xzz
type Air = { temperature_c: float }
v raw = load("air.csv") :: Air
v warm = raw |> filter(temperture_c > 20)
```

```text
❌ [error] expression: column 'temperture_c' does not exist in the schema.
💡 available columns: temperature_c
  Did you mean: col("temperature_c")?
```

## Undeclared column in `select`

`column.xzz`:

```xzz
type Air = { pm10: float }
v raw = load("air.csv") :: Air
v selected = raw |> select(["missing"])
```

```text
❌ [error] select: column 'missing' does not exist in the schema.
💡 available columns: pm10
  Did you mean: col("pm10")?
```

## Invalid cast type

`cast.xzz`:

```xzz
type Air = { pm10: float }
v raw = load("air.csv") :: Air
v bad = raw |> cast("pm10", "decimal")
```

```text
❌ [error] cast("pm10", "decimal") : unknown type 'decimal'. Supported types: "float", "int", "str", "bool"
```

## `Option<T>`: `fillNull` on a non-nullable column

`option.xzz`:

```xzz
type Air = { pm10: float }
v raw = load("air.csv") :: Air
v filled = raw |> fillNull("pm10", strategy: "mean")
```

```text
❌ [error] fillNull("pm10", ...) : column 'pm10' is declared as a non-nullable type. Declare 'pm10' as Option<float> in the schema, or remove this operation.
```

Declare `pm10: Option<float>` if empty values are expected, or remove the
`fillNull` operation when the data cannot be null.
