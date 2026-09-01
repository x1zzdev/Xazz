//! xazz-exec/src/chart.rs — visualization (chart) subsystem (v0.19)
//!
//! Responsible for DataFrame → JSON chart spec → Chart.js HTML rendering.
//! The runtime (runtime.rs) only calls this module's functions when creating charts
//! and does not hold visualization knowledge itself (God runtime dismantled).

use serde::Serialize;

use xazz_compiler::ast::{ChartConfig, ChartType};

// ─────────────────────────────────────────────────────────────────────────────
// ── ChartSpec — visualization spec passed to the frontend (v0.19) ────────────
// ─────────────────────────────────────────────────────────────────────────────

/// Recharts-compatible JSON chart spec
#[derive(Debug, Serialize)]
pub struct ChartSpec {
    #[serde(rename = "chartType")]
    pub chart_type: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub data: serde_json::Value,
}

// ── DataFrame → ChartSpec conversion ──────────────────────────────────────────
pub fn build_chart_spec(
    config: &ChartConfig,
    df: &polars::frame::DataFrame,
) -> Result<ChartSpec, Box<dyn std::error::Error>> {
    let check_col = |col_name: &str| -> Result<(), Box<dyn std::error::Error>> {
        if df.column(col_name).is_err() {
            let cols: Vec<String> = df
                .get_column_names()
                .iter()
                .map(|s| s.to_string())
                .collect();
            Err(format!(
                "ERROR[VIZ001]: Column '{}' not found. 사용 가능한 컬럼: {}",
                col_name,
                cols.join(", ")
            )
            .into())
        } else {
            Ok(())
        }
    };

    match &config.chart_type {
        ChartType::Bar | ChartType::Line | ChartType::Scatter => {
            if let Some(ref x) = config.x {
                check_col(x)?;
            }
            if let Some(ref y) = config.y {
                check_col(y)?;
            }
        }
        ChartType::Pie => {
            if let Some(ref l) = config.label {
                check_col(l)?;
            }
            if let Some(ref v) = config.value {
                check_col(v)?;
            }
        }
    }

    let data = df_to_json_array(df)?;

    Ok(ChartSpec {
        chart_type: config.chart_type.as_str().to_string(),
        title: config.title.clone().unwrap_or_default(),
        x: config.x.clone(),
        y: config.y.clone(),
        label: config.label.clone(),
        value: config.value.clone(),
        data,
    })
}

/// Serializes a Polars DataFrame to a JSON array (`[{col: val, ...}, ...]`)
pub fn df_to_json_array(
    df: &polars::frame::DataFrame,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    use polars::prelude::AnyValue;

    let col_names: Vec<String> = df
        .get_column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let height = df.height();
    let mut rows: Vec<serde_json::Value> = Vec::with_capacity(height);

    for row_idx in 0..height {
        let mut obj = serde_json::Map::new();
        for col_name in &col_names {
            if let Ok(series) = df.column(col_name) {
                let val = match series.get(row_idx) {
                    Ok(AnyValue::Null) => serde_json::Value::Null,
                    Ok(AnyValue::Boolean(b)) => serde_json::Value::Bool(b),
                    Ok(AnyValue::Int8(n)) => serde_json::json!(n),
                    Ok(AnyValue::Int16(n)) => serde_json::json!(n),
                    Ok(AnyValue::Int32(n)) => serde_json::json!(n),
                    Ok(AnyValue::Int64(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt8(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt16(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt32(n)) => serde_json::json!(n),
                    Ok(AnyValue::UInt64(n)) => serde_json::json!(n),
                    Ok(AnyValue::Float32(f)) => serde_json::json!(f),
                    Ok(AnyValue::Float64(f)) => serde_json::json!(f),
                    Ok(AnyValue::String(s)) => serde_json::Value::String(s.to_string()),
                    Ok(AnyValue::StringOwned(s)) => serde_json::Value::String(s.to_string()),
                    Ok(other) => serde_json::Value::String(format!("{}", other)),
                    Err(_) => serde_json::Value::Null,
                };
                obj.insert(col_name.to_string(), val);
            }
        }
        rows.push(serde_json::Value::Object(obj));
    }

    Ok(serde_json::Value::Array(rows))
}

// ── write_chart_html — ChartSpec → Chart.js-based HTML file generation ────────
pub fn write_chart_html(
    spec: &ChartSpec,
    output_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let data_json = serde_json::to_string(&spec.data)?;
    // JSON is inlined inside a script tag, so escape `<`, `>`, `&`, U+2028/U+2029
    // as \uXXXX to prevent escaping the script block via `</script>`.
    // (serde_json does not escape `<`/`>` — stored XSS prevention)
    let data_json_escaped = data_json
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let title = &spec.title;
    let chart_type_str = &spec.chart_type;

    // JSON-encode for safe insertion into JS strings/keys. (stored XSS prevention)
    let js = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string());
    // Escape for the HTML context (title)
    let html_esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };

    let title_json = js(title);
    let title_html = html_esc(title);

    let chartjs_type = match chart_type_str.as_str() {
        "bar" => "bar",
        "line" => "line",
        "pie" => "pie",
        "scatter" => "scatter",
        other => other,
    };
    let chartjs_type_json = js(chartjs_type);

    let dataset_js = match chart_type_str.as_str() {
        "pie" => {
            let label_field = spec.label.as_deref().unwrap_or("label");
            let value_field = spec.value.as_deref().unwrap_or("value");
            let label_field_json = js(label_field);
            let value_field_json = js(value_field);
            format!(
                r#"{{
            type: {chartjs_type_json},
            data: {{
                labels: data.map(d => d[{label_field_json}]),
                datasets: [{{
                    label: {title_json},
                    data: data.map(d => d[{value_field_json}]),
                    backgroundColor: [
                        'rgba(255, 99, 132, 0.7)',
                        'rgba(54, 162, 235, 0.7)',
                        'rgba(255, 206, 86, 0.7)',
                        'rgba(75, 192, 192, 0.7)',
                        'rgba(153, 102, 255, 0.7)',
                        'rgba(255, 159, 64, 0.7)',
                        'rgba(199, 199, 199, 0.7)',
                        'rgba(83, 102, 255, 0.7)',
                        'rgba(40, 159, 64, 0.7)',
                        'rgba(210, 99, 132, 0.7)'
                    ],
                    borderWidth: 1
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{
                    legend: {{ display: true, position: 'right' }},
                    title: {{ display: false }}
                }}
            }}
        }}"#,
                chartjs_type_json = chartjs_type_json,
                label_field_json = label_field_json,
                value_field_json = value_field_json,
                title_json = title_json,
            )
        }
        "scatter" => {
            let x_field = spec.x.as_deref().unwrap_or("x");
            let y_field = spec.y.as_deref().unwrap_or("y");
            let x_field_json = js(x_field);
            let y_field_json = js(y_field);
            format!(
                r#"{{
            type: {chartjs_type_json},
            data: {{
                datasets: [{{
                    label: {title_json},
                    data: data.map(d => ({{ x: d[{x_field_json}], y: d[{y_field_json}] }})),
                    backgroundColor: 'rgba(54, 162, 235, 0.5)',
                    borderColor: 'rgba(54, 162, 235, 1)',
                    pointRadius: 5
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{ legend: {{ display: true }} }},
                scales: {{
                    x: {{ title: {{ display: true, text: {x_field_json} }} }},
                    y: {{ title: {{ display: true, text: {y_field_json} }}, beginAtZero: false }}
                }}
            }}
        }}"#,
                chartjs_type_json = chartjs_type_json,
                title_json = title_json,
                x_field_json = x_field_json,
                y_field_json = y_field_json,
            )
        }
        _ => {
            let x_field = spec.x.as_deref().unwrap_or("x");
            let y_field = spec.y.as_deref().unwrap_or("y");
            let x_field_json = js(x_field);
            let y_field_json = js(y_field);
            let bg_color = if chart_type_str == "line" {
                "rgba(54, 162, 235, 0.1)"
            } else {
                "rgba(54, 162, 235, 0.5)"
            };
            let border_fill = if chart_type_str == "line" {
                "true"
            } else {
                "false"
            };
            format!(
                r#"{{
            type: {chartjs_type_json},
            data: {{
                labels: data.map(d => d[{x_field_json}]),
                datasets: [{{
                    label: {title_json},
                    data: data.map(d => d[{y_field_json}]),
                    backgroundColor: '{bg_color}',
                    borderColor: 'rgba(54, 162, 235, 1)',
                    borderWidth: 2,
                    fill: {border_fill}
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{ legend: {{ display: true }} }},
                scales: {{
                    x: {{ title: {{ display: true, text: {x_field_json} }} }},
                    y: {{ beginAtZero: true, title: {{ display: true, text: {y_field_json} }} }}
                }}
            }}
        }}"#,
                chartjs_type_json = chartjs_type_json,
                x_field_json = x_field_json,
                y_field_json = y_field_json,
                title_json = title_json,
                bg_color = bg_color,
                border_fill = border_fill,
            )
        }
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title_html}</title>
  <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      display: flex;
      justify-content: center;
      align-items: center;
      min-height: 100vh;
      background: #f0f2f5;
      font-family: 'Segoe UI', sans-serif;
    }}
    .chart-container {{
      width: 900px;
      max-width: 95vw;
      background: white;
      border-radius: 16px;
      padding: 32px;
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.12);
    }}
    h1 {{
      text-align: center;
      color: #1a1a2e;
      font-size: 1.5em;
      margin-bottom: 24px;
      font-weight: 600;
    }}
    .meta {{
      text-align: center;
      color: #888;
      font-size: 0.8em;
      margin-top: 16px;
    }}
  </style>
</head>
<body>
  <div class="chart-container">
    <h1>{title_html}</h1>
    <canvas id="xazz-chart"></canvas>
    <p class="meta">Generated by xazz-lang 📊</p>
  </div>
  <script>
    const data = {data_json_escaped};
    new Chart(document.getElementById('xazz-chart'), {dataset_js});
  </script>
</body>
</html>
"#,
        title_html = title_html,
        data_json_escaped = data_json_escaped,
        dataset_js = dataset_js,
    );

    std::fs::write(output_path, html)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use xazz_compiler::ast::{ChartConfig, ChartType};

    fn spec_with_cell(cell: &str) -> ChartSpec {
        let data = serde_json::json!([{"label": cell, "value": 1}]);
        ChartSpec {
            chart_type: "bar".into(),
            title: "t".into(),
            x: Some("label".into()),
            y: Some("value".into()),
            label: None,
            value: None,
            data,
        }
    }

    #[test]
    fn chart_html_escapes_script_closing_tag_in_data() {
        let out = std::env::temp_dir().join("xazz_xss_test.html");
        write_chart_html(
            &spec_with_cell("</script><script>alert(1)</script>"),
            out.to_str().unwrap(),
        )
        .unwrap();
        let html = std::fs::read_to_string(&out).unwrap();
        assert!(
            !html.contains("</script><script>alert"),
            "raw </script> must not be inserted verbatim"
        );
        assert!(
            html.contains("\\u003c/script\\u003e"),
            "< must be escaped as \\u003c"
        );
        std::fs::remove_file(&out).ok();
    }

    #[test]
    fn chart_html_keeps_normal_data_intact() {
        let out = std::env::temp_dir().join("xazz_xss_ok_test.html");
        write_chart_html(&spec_with_cell("Gangnam"), out.to_str().unwrap()).unwrap();
        let html = std::fs::read_to_string(&out).unwrap();
        assert!(html.contains("Gangnam"));
        std::fs::remove_file(&out).ok();
    }
}
