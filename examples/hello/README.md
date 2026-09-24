# Hello Xazz

A small pipeline using only built-in operators: load a CSV, remove non-positive
amounts, sum sales by category, and draw a bar chart. No service or database is
needed.

From the repository root, build the CLI and its two execution binaries:

```bash
cargo build -p xazz -p xazz-runner -p xazz-exec
cd examples/hello
../../target/debug/xazz check main.xzz
../../target/debug/xazz run main.xzz
```

If the binaries are already installed together, use `xazz check main.xzz` and
`xazz run main.xzz`. Run from this directory because `load("sales.csv")` uses the
current working directory. The check should report zero errors. The run writes
`chart_by_category_chart.html` here. Open it in a browser to see two bars:
**Food = 12** and **Stationery = 15**. The negative refund row is filtered out.

The chart page loads Chart.js from a CDN, so viewing it requires internet access.
