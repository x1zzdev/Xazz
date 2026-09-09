// xazz-compiler/src/catalog.rs — pipeline catalog + column lineage (issue C3)
//
// Given a `TypedProgram`, builds:
//   1. A **pipeline catalog** — each pipeline's id, variable name, and its
//      input/output column names (with types). This is the "what ran" surface.
//   2. **Column lineage** — for each output column, which source column(s) it
//      derives from. A reviewer can trace `groupBy → agg → chart` output
//      columns back to the source columns.
//
// The lineage is a per-pipeline graph computed by walking the ordered `steps`
// and tracking each current column's provenance. Data-domain ops that rename,
// select, aggregate, or add columns update provenance; pass-through ops
// (filter/drop-null/cast/sort/limit/sample/replace) preserve it.

use crate::ir::{DataOp, PipelineNode, TypedExprKind, TypedProgram};
use serde::Serialize;

/// A column's lineage — where this column comes from.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnLineage {
    /// Current (output) column name.
    pub column: String,
    /// Source column name(s) this column derives from (empty = derived/constant).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<String>,
    /// Pipeline-local step index that created it (0 = source).
    pub created_by_step: usize,
}

/// Catalog entry for one pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct PipelineCatalogEntry {
    pub id: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Input columns (source → first op).
    pub input_columns: Vec<String>,
    /// Output columns (terminal).
    pub output_columns: Vec<String>,
    /// Lineage: output column → source column(s).
    pub lineage: Vec<ColumnLineage>,
}

/// The whole-program catalog.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Catalog {
    pub pipelines: Vec<PipelineCatalogEntry>,
}

/// Builds the catalog for a typed program.
pub fn build_catalog(program: &TypedProgram) -> Catalog {
    Catalog {
        pipelines: program.pipelines.iter().map(build_pipeline_entry).collect(),
    }
}

/// Builds a single pipeline's catalog entry by walking its steps.
fn build_pipeline_entry(node: &PipelineNode) -> PipelineCatalogEntry {
    // provenance: current column → source column(s)
    let mut provenance: Vec<(String, Vec<String>, usize)> = Vec::new();
    let mut step_idx: usize = 0;

    // Seed with input schema columns (source = themselves).
    if let Some(schema) = &node.input_schema {
        for f in &schema.fields {
            provenance.push((f.name.clone(), vec![f.name.clone()], 0));
        }
    }

    for step in &node.steps {
        step_idx += 1;
        if let crate::ir::Step::Data(op) = step {
            apply_data_op(op, &mut provenance, step_idx);
        }
    }

    let output_columns: Vec<String> = provenance.iter().map(|(c, _, _)| c.clone()).collect();
    let lineage = provenance
        .iter()
        .map(|(c, from, s)| ColumnLineage {
            column: c.clone(),
            from: from.clone(),
            created_by_step: *s,
        })
        .collect();

    PipelineCatalogEntry {
        id: node.id,
        name: node.name.clone(),
        input_columns: node
            .input_schema
            .as_ref()
            .map(|s| s.fields.iter().map(|f| f.name.clone()).collect())
            .unwrap_or_default(),
        output_columns,
        lineage,
    }
}

/// Applies a data op to the provenance map.
fn apply_data_op(
    op: &DataOp,
    provenance: &mut Vec<(String, Vec<String>, usize)>,
    step_idx: usize,
) {
    use DataOp::*;
    match op {
        // Pass-through ops — provenance unchanged.
        Filter(_) | DropNull(_) | Sort { .. } | Limit(_) | Sample { .. } | Cast { .. }
        | Replace { .. } | FillNull { .. } => {}

        // Select — keep only the chosen columns, preserving their provenance.
        Select(cols) => {
            provenance.retain(|(c, _, _)| cols.contains(c));
        }

        // Rename — old → new, provenance carried over.
        Rename { old, new } => {
            if let Some(idx) = provenance.iter().position(|(c, _, _)| c == old) {
                let (_, from, s) = provenance.remove(idx);
                provenance.push((new.clone(), from, s));
            }
        }

        // WithColumn — add/replace a derived column. Lineage = referenced columns.
        WithColumn { name, expr } => {
            let refs = expr_column_refs(expr);
            // Replace if the name already exists.
            if let Some(idx) = provenance.iter().position(|(c, _, _)| c == name) {
                provenance.remove(idx);
            }
            provenance.push((name.clone(), refs, step_idx));
        }

        // GroupBy — the group key column stays; other columns are dropped
        // (a later Aggregate re-adds the aggregated column).
        GroupBy(col) => {
            provenance.retain(|(c, _, _)| c == col);
        }

        // Aggregate — the aggregated column derives from `col` (as a statistic).
        Aggregate { col, .. } => {
            // If the column already exists (group key) keep it; the aggregate
            // column's lineage = the source column it summarizes.
            if !provenance.iter().any(|(c, _, _)| c == col) {
                provenance.push((col.clone(), vec![col.clone()], step_idx));
            }
        }

        // Join — merge the other side's columns. We don't have the other side's
        // schema here (it's a variable ref), so provenance for joined columns
        // is best-effort: keep existing columns, mark join keys.
        Join { left_on, right_on, .. } => {
            for k in left_on {
                if !provenance.iter().any(|(c, _, _)| c == k) {
                    provenance.push((k.clone(), vec![k.clone()], step_idx));
                }
            }
            let _ = right_on;
        }
    }
}

/// Extracts the column references from a typed expression.
fn expr_column_refs(expr: &crate::ir::TypedExpr) -> Vec<String> {
    fn walk(e: &crate::ir::TypedExpr, out: &mut Vec<String>) {
        match &e.kind {
            TypedExprKind::Column(c) => out.push(c.clone()),
            TypedExprKind::BinOp { lhs, rhs, .. } => {
                walk(lhs, out);
                walk(rhs, out);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(expr, &mut out);
    out
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ColType, Schema, SchemaField, Step, TypedExpr, TypedExprKind};

    fn schema(cols: &[(&str, ColType)]) -> Schema {
        Schema {
            fields: cols
                .iter()
                .map(|(n, t)| SchemaField::new(*n, t.clone()))
                .collect(),
        }
    }

    fn node(id: usize, input: Schema, steps: Vec<Step>) -> PipelineNode {
        PipelineNode {
            id,
            name: Some(format!("p{id}")),
            source: crate::ir::Source::Load {
                file_path: "d.csv".into(),
                schema: None,
            },
            input_schema: Some(input),
            output_schema: Schema::default(),
            steps,
            yields_model: false,
        }
    }

    fn d(op: DataOp) -> Step {
        Step::Data(op)
    }

    #[test]
    fn select_and_rename_track_lineage() {
        let input = schema(&[
            ("region", ColType::String),
            ("pm10", ColType::Float),
            ("pm25", ColType::Float),
        ]);
        let p = node(
            0,
            input,
            vec![
                d(DataOp::Select(vec!["region".into(), "pm10".into()])),
                d(DataOp::Rename {
                    old: "pm10".into(),
                    new: "pm10_clean".into(),
                }),
            ],
        );
        let prog = TypedProgram {
            pipelines: vec![p],
            ..Default::default()
        };
        let cat = build_catalog(&prog);
        let entry = &cat.pipelines[0];
        assert_eq!(entry.output_columns, vec!["region", "pm10_clean"]);
        // pm10_clean lineage → pm10
        let l = entry
            .lineage
            .iter()
            .find(|l| l.column == "pm10_clean")
            .unwrap();
        assert_eq!(l.from, vec!["pm10"]);
    }

    #[test]
    fn group_by_agg_lineage_points_to_source() {
        let input = schema(&[
            ("station", ColType::String),
            ("pm10", ColType::Float),
            ("date", ColType::String),
        ]);
        let p = node(
            0,
            input,
            vec![
                d(DataOp::GroupBy("station".into())),
                d(DataOp::Aggregate {
                    kind: crate::ir::AggKind::Mean,
                    col: "pm10".into(),
                }),
            ],
        );
        let prog = TypedProgram {
            pipelines: vec![p],
            ..Default::default()
        };
        let cat = build_catalog(&prog);
        let entry = &cat.pipelines[0];
        // group key + aggregate column
        assert!(entry.output_columns.contains(&"station".to_string()));
        assert!(entry.output_columns.contains(&"pm10".to_string()));
        // aggregated pm10 lineage → pm10 (source)
        let l = entry.lineage.iter().find(|l| l.column == "pm10").unwrap();
        assert_eq!(l.from, vec!["pm10"]);
        // date dropped after groupBy
        assert!(!entry.output_columns.contains(&"date".to_string()));
    }

    #[test]
    fn with_column_derives_from_referenced_columns() {
        let input = schema(&[("temp", ColType::Float), ("hum", ColType::Float)]);
        let p = node(
            0,
            input,
            vec![d(DataOp::WithColumn {
                name: "heat_index".into(),
                expr: TypedExpr::new(
                    TypedExprKind::BinOp {
                        op: crate::ast::BinOpKind::Add,
                        lhs: Box::new(TypedExpr::new(
                            TypedExprKind::Column("temp".into()),
                            ColType::Float,
                        )),
                        rhs: Box::new(TypedExpr::new(
                            TypedExprKind::Column("hum".into()),
                            ColType::Float,
                        )),
                    },
                    ColType::Float,
                ),
            })],
        );
        let prog = TypedProgram {
            pipelines: vec![p],
            ..Default::default()
        };
        let cat = build_catalog(&prog);
        let l = cat.pipelines[0]
            .lineage
            .iter()
            .find(|l| l.column == "heat_index")
            .unwrap();
        let mut from = l.from.clone();
        from.sort();
        assert_eq!(from, vec!["hum", "temp"]);
    }
}