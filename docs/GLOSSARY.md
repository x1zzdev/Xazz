# Xazz glossary

Short definitions for terms used in the README and architecture documents.

| Term | Meaning | Read more |
|---|---|---|
| `.xzz` | A Xazz source file describing a data and model pipeline. | [Quick Start](../README.md#quick-start) |
| Pipeline | The ordered operations that load, transform, train on, or emit data. | [Architecture](ARCHITECTURE.md#compilation-pipeline) |
| Lexer | The compiler stage that turns source characters into tokens. | [Architecture](ARCHITECTURE.md#compilation-pipeline) |
| Parser | The stage that turns tokens into an abstract syntax tree. | [Architecture](ARCHITECTURE.md#compilation-pipeline) |
| AST | Abstract syntax tree: the parsed structure of a `.xzz` program before type checking. | [Architecture](ARCHITECTURE.md#compilation-pipeline) |
| Type | A constraint on values or columns, such as numeric, text, or optional. | [Type system](ARCHITECTURE.md#type-system) |
| `Option<T>` | A value of type `T` that may be absent; the checker requires explicit handling. | [Null safety](ARCHITECTURE.md#null-safety) |
| Schema | The names and types of columns available at a pipeline step. | [Column types](ARCHITECTURE.md#column-types) |
| Static checker | The compiler pass that detects invalid names, types, and unsafe operations before execution. | [Type system](ARCHITECTURE.md#type-system) |
| Typed IR | Typed intermediate representation: the checked program form consumed by the runtime. | [Typed IR](ARCHITECTURE.md#typed-ir-xazz-coreir) |
| Lowering | Translation from typed data operations into Polars operations for execution. | [Execution model](ARCHITECTURE.md#execution-model) |
| Lazy execution | Deferring data work until a result is needed so Polars can optimize the operation chain. | [Execution model](ARCHITECTURE.md#execution-model) |
| Polars | The Rust data-frame engine used for tabular operations in `xazz-exec`. | [Workspace](WORKSPACE.md#crate-responsibilities) |
| Burn | The Rust machine-learning engine used for training and model operations. | [Workspace](WORKSPACE.md#crate-responsibilities) |
| Tensor bridge | The conversion path from Polars/Arrow column buffers to Burn tensors, with remaining copies documented. | [Memory model](ARCHITECTURE.md#memory-model-polars--burn) |
| Differential privacy (DP) | A way to add calibrated noise to eligible outputs while tracking privacy cost. | [Differential privacy](ARCHITECTURE.md#differential-privacy) |
| Privacy budget (`ε`, `δ`) | Limits on cumulative DP consumption; lower `ε` generally means stronger privacy. | [DP design](design/dp-spec.md) |
| Guardrail | A policy check that warns or blocks an unsafe pipeline before it runs. | [Security guardrail](SECURITY_GUARDRAIL.md) |
| Policy pack | A set of guardrail rules and thresholds applied to a pipeline or tenant. | [Policy files](SECURITY_GUARDRAIL.md#5-policy-files) |
| Audit hash chain | Run records linked by hashes so later modification can be detected. | [Audit and results markers](ARCHITECTURE.md#audit--results-markers) |

