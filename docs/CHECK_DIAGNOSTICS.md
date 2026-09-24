# `xazz check` diagnostic gallery

These four snippets fail before any CSV is opened. Save each as the indicated
`.xzz` file and run `XAZZ_LANG=en xazz check <file>`. The commands below were
reproduced with the current CLI; each exits 1 with one error.

The plain-text and `--json` output both report a 1-based `line:col` location.
The output excerpts below omit the file summary and final error count. For
three cases, column 1 identifies the start of the offending statement.

| Case | Reported `line:col` |
|---|---:|
| Did-you-mean | `3:24` |
| Undeclared column | `3:1` |
| Invalid cast type | `3:1` |
| Non-nullable `fillNull` | `3:1` |

## Did-you-mean: mistyped column

`typo.xzz`:

```xzz
type Air = { temperature_c: float }
v raw = load("air.csv") :: Air
v warm = raw |> filter(temperture_c > 20)
```

```text
❌ [error [line 3: col 24]] expression: column 'temperture_c' does not exist in the schema.
   💡 Schema 'expression' does not contain column 'temperture_c'.
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
❌ [error [line 3: col 1]] select: column 'missing' does not exist in the schema.
   💡 Schema 'select' does not contain column 'missing'.
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
❌ [error [line 3: col 1]] cast("pm10", "decimal") : unknown type 'decimal'. Supported types: "float", "int", "str", "bool"
```

## `Option<T>`: `fillNull` on a non-nullable column

`option.xzz`:

```xzz
type Air = { pm10: float }
v raw = load("air.csv") :: Air
v filled = raw |> fillNull("pm10", strategy: "mean")
```

```text
❌ [error [line 3: col 1]] fillNull("pm10", ...) : column 'pm10' is declared as a non-nullable type. Declare 'pm10' as Option<float> in the schema, or remove this operation.
```

Declare `pm10: Option<float>` if empty values are expected, or remove the
`fillNull` operation when the data cannot be null.
