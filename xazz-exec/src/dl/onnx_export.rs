//! xazz-exec/src/dl/onnx_export.rs — ONNX export + inference (D2 #63).
//!
//! Exports the trained CPU [`Mlp`](super::Mlp) graph to a standard ONNX
//! `ModelProto` and runs it through ONNX Runtime (`ort`). This is a child module
//! of `dl` so it can read the model's private graph, weights, and preprocessing
//! helpers without widening their visibility elsewhere.
//!
//! The artifact is produced from the CPU reference model, so the same weights
//! that `predict` evaluates in-memory can be evaluated by ONNX Runtime and their
//! predictions compared (the D2 acceptance test).
//!
//! Supported graph ops mirror the DSL layers: `Dense`, `Conv1d` (Same padding),
//! `Embedding`, and the ReLU/Sigmoid/Tanh/Softmax activations (Dropout is the
//! identity at inference). The exporter fails closed on anything it cannot map.

use burn::tensor::Tensor;
use ort::ep::ExecutionProviderDispatch;
use polars::prelude::DataFrame;
use protobuf::Message;
use rlx_onnx_proto::onnx;
use xazz_core::i18n::tr;

use crate::backend::{DeviceSpec, OrtEpKind, OrtEpSpec, parse_device_spec, parse_ort_ep_spec};

use super::{Activation, LayerOp, Plain, TrainedModel};

/// ONNX opset targeted by the exporter.
const OPSET: i64 = 13;

fn onnx_err<E: std::fmt::Display>(e: E) -> String {
    format!("ONNX: {e}")
}

/// Extracts `(shape, data)` from a CPU tensor.
fn tensor_parts<const D: usize>(t: Tensor<Plain, D>) -> Result<(Vec<usize>, Vec<f32>), String> {
    let data = t.into_data();
    let shape: Vec<usize> = data.shape.iter().copied().collect();
    let values = data
        .to_vec::<f32>()
        .map_err(|e| format!("ONNX: model weight is not f32: {e}"))?;
    Ok((shape, values))
}

fn tensor_proto_f32(name: &str, dims: &[usize], data: Vec<f32>) -> onnx::TensorProto {
    let mut t = onnx::TensorProto::new();
    t.set_name(name.to_string());
    t.set_data_type(onnx::TensorProto_DataType::FLOAT);
    t.set_dims(dims.iter().map(|&d| d as i64).collect());
    t.set_float_data(data);
    t
}

fn tensor_proto_i64(name: &str, dims: &[usize], data: Vec<i64>) -> onnx::TensorProto {
    let mut t = onnx::TensorProto::new();
    t.set_name(name.to_string());
    t.set_data_type(onnx::TensorProto_DataType::INT64);
    t.set_dims(dims.iter().map(|&d| d as i64).collect());
    t.set_int64_data(data);
    t
}

/// A graph input/output descriptor. `batch_dynamic` prepends ONNX's symbolic
/// batch dimension `N` before the fixed `dims`.
fn value_info(
    name: &str,
    batch_dynamic: bool,
    dims: &[usize],
    elem: onnx::TensorProto_DataType,
) -> onnx::ValueInfoProto {
    let mut shape = onnx::TensorShapeProto::new();
    if batch_dynamic {
        let mut dim = onnx::TensorShapeProto_Dimension::new();
        dim.set_dim_param("N".to_string());
        shape.mut_dim().push(dim);
    }
    for &d in dims {
        let mut dim = onnx::TensorShapeProto_Dimension::new();
        dim.set_dim_value(d as i64);
        shape.mut_dim().push(dim);
    }
    let mut tensor_type = onnx::TypeProto_Tensor::new();
    tensor_type.set_elem_type(elem);
    tensor_type.set_shape(shape);
    let mut type_proto = onnx::TypeProto::new();
    type_proto.set_tensor_type(tensor_type);
    let mut vi = onnx::ValueInfoProto::new();
    vi.set_name(name.to_string());
    vi.set_field_type(type_proto);
    vi
}

fn make_node(op: &str, inputs: Vec<String>, outputs: Vec<String>) -> onnx::NodeProto {
    let mut n = onnx::NodeProto::new();
    n.set_op_type(op.to_string());
    n.set_input(inputs.into_iter().collect());
    n.set_output(outputs.into_iter().collect());
    n
}

fn attr_int(name: &str, v: i64) -> onnx::AttributeProto {
    let mut a = onnx::AttributeProto::new();
    a.set_name(name.to_string());
    a.set_field_type(onnx::AttributeProto_AttributeType::INT);
    a.set_i(v);
    a
}

fn attr_ints(name: &str, v: Vec<i64>) -> onnx::AttributeProto {
    let mut a = onnx::AttributeProto::new();
    a.set_name(name.to_string());
    a.set_field_type(onnx::AttributeProto_AttributeType::INTS);
    a.set_ints(v);
    a
}

/// Emits the activation node after an op, returning the new current tensor name.
fn apply_activation(
    act: &Activation,
    cur_name: String,
    tag: &str,
    nodes: &mut Vec<onnx::NodeProto>,
) -> String {
    let (op, axis): (&str, Option<i64>) = match act {
        Activation::None | Activation::Dropout(_) => return cur_name,
        Activation::ReLU => ("Relu", None),
        Activation::Sigmoid => ("Sigmoid", None),
        Activation::Tanh => ("Tanh", None),
        Activation::Softmax => ("Softmax", Some(1)),
    };
    let out = format!("act{tag}");
    let mut node = make_node(op, vec![cur_name], vec![out.clone()]);
    if let Some(axis) = axis {
        node.mut_attribute().push(attr_int("axis", axis));
    }
    nodes.push(node);
    out
}

/// Builds the ONNX model for the trained graph.
fn build_model(trained: &TrainedModel) -> Result<onnx::ModelProto, String> {
    let model = &trained.model;
    let input_dim = trained.report.input_dim;

    let mut nodes: Vec<onnx::NodeProto> = Vec::new();
    let mut inits: Vec<onnx::TensorProto> = Vec::new();
    let mut cur = input_dim;
    let mut prev = "input".to_string();

    for (tag, (op, act)) in model.ops.iter().enumerate() {
        let out = match op {
            LayerOp::Dense(i) => {
                let lin = &model.linears[*i];
                let (w_shape, w) = tensor_parts(lin.weight.val())?;
                if w_shape.len() != 2 {
                    return Err("ONNX: Dense weight is not rank-2".to_string());
                }
                let units = w_shape[1];
                let w_name = format!("w{tag}");
                inits.push(tensor_proto_f32(&w_name, &w_shape, w));

                let mut inputs = vec![prev.clone(), w_name];
                if let Some(bias) = &lin.bias {
                    let (b_shape, b) = tensor_parts(bias.val())?;
                    let b_name = format!("b{tag}");
                    inits.push(tensor_proto_f32(&b_name, &b_shape, b));
                    inputs.push(b_name);
                }
                let out = format!("d{tag}");
                let mut gemm = make_node("Gemm", inputs, vec![out.clone()]);
                gemm.mut_attribute().push(attr_int("transB", 0));
                nodes.push(gemm);
                cur = units;
                out
            }
            LayerOp::Conv1d(i) => {
                let conv = &model.convs[*i];
                let (w_shape, w) = tensor_parts(conv.weight.val())?;
                if w_shape.len() != 3 {
                    return Err("ONNX: Conv1d weight is not rank-3".to_string());
                }
                let (out_ch, kernel) = (w_shape[0], w_shape[2]);
                let w_name = format!("cw{tag}");
                inits.push(tensor_proto_f32(&w_name, &w_shape, w));
                let mut conv_inputs = vec![format!("cr{tag}"), w_name];
                if let Some(bias) = &conv.bias {
                    let (b_shape, b) = tensor_parts(bias.val())?;
                    let b_name = format!("cb{tag}");
                    inits.push(tensor_proto_f32(&b_name, &b_shape, b));
                    conv_inputs.push(b_name);
                }

                // [N, cur] -> [N, 1, cur]
                let shape_in = format!("cshape_in{tag}");
                inits.push(tensor_proto_i64(&shape_in, &[3], vec![-1, 1, cur as i64]));
                nodes.push(make_node(
                    "Reshape",
                    vec![prev.clone(), shape_in],
                    vec![format!("cr{tag}")],
                ));

                // Same padding (stride 1): total = kernel - 1, split like Burn.
                let total = kernel.saturating_sub(1);
                let pads = vec![(total / 2) as i64, (total - total / 2) as i64];
                let conv_out = format!("cc{tag}");
                let mut conv_node = make_node("Conv", conv_inputs, vec![conv_out.clone()]);
                conv_node.mut_attribute().push(attr_ints("pads", pads));
                nodes.push(conv_node);

                // [N, out, cur] -> [N, out * cur]
                let shape_out = format!("cshape_out{tag}");
                inits.push(tensor_proto_i64(
                    &shape_out,
                    &[2],
                    vec![-1, (out_ch * cur) as i64],
                ));
                let out = format!("d{tag}");
                nodes.push(make_node(
                    "Reshape",
                    vec![conv_out, shape_out],
                    vec![out.clone()],
                ));
                cur *= out_ch;
                out
            }
            LayerOp::Embedding(i) => {
                let emb = &model.embeddings[*i];
                let (w_shape, w) = tensor_parts(emb.weight.val())?;
                if w_shape.len() != 2 {
                    return Err("ONNX: Embedding weight is not rank-2".to_string());
                }
                let embed_dim = w_shape[1];
                let w_name = format!("ew{tag}");
                inits.push(tensor_proto_f32(&w_name, &w_shape, w));

                let vocabs = &model.embed_vocab[*i];
                let offsets = &model.embed_offsets[*i];
                let len = cur;

                // Split the [N, len] index row into one [N, 1] column per feature.
                let cols: Vec<String> = if len == 1 {
                    vec![prev.clone()]
                } else {
                    let split_name = format!("esplit{tag}");
                    inits.push(tensor_proto_i64(&split_name, &[len], vec![1; len]));
                    let outs: Vec<String> = (0..len).map(|j| format!("ec{tag}_{j}")).collect();
                    let mut split =
                        make_node("Split", vec![prev.clone(), split_name], outs.clone());
                    split.mut_attribute().push(attr_int("axis", 1));
                    nodes.push(split);
                    outs
                };

                let mut index_cols: Vec<String> = Vec::with_capacity(len);
                for (j, col) in cols.iter().enumerate() {
                    let vocab = vocabs[j];
                    let min_name = format!("emin{tag}_{j}");
                    let max_name = format!("emax{tag}_{j}");
                    inits.push(tensor_proto_f32(&min_name, &[], vec![0.0]));
                    inits.push(tensor_proto_f32(
                        &max_name,
                        &[],
                        vec![vocab.saturating_sub(1) as f32],
                    ));
                    let clipped = format!("eclip{tag}_{j}");
                    nodes.push(make_node(
                        "Clip",
                        vec![col.clone(), min_name, max_name],
                        vec![clipped.clone()],
                    ));

                    if offsets[j] != 0 {
                        let off_name = format!("eoff{tag}_{j}");
                        inits.push(tensor_proto_f32(&off_name, &[], vec![offsets[j] as f32]));
                        let added = format!("eadd{tag}_{j}");
                        nodes.push(make_node(
                            "Add",
                            vec![clipped, off_name],
                            vec![added.clone()],
                        ));
                        index_cols.push(added);
                    } else {
                        index_cols.push(clipped);
                    }
                }

                let cat = if index_cols.len() == 1 {
                    index_cols.remove(0)
                } else {
                    let cat = format!("ecat{tag}");
                    let mut concat = make_node("Concat", index_cols, vec![cat.clone()]);
                    concat.mut_attribute().push(attr_int("axis", 1));
                    nodes.push(concat);
                    cat
                };
                let idx = format!("eidx{tag}");
                let mut cast = make_node("Cast", vec![cat], vec![idx.clone()]);
                cast.mut_attribute()
                    .push(attr_int("to", onnx::TensorProto_DataType::INT64 as i64));
                nodes.push(cast);

                let gathered = format!("eg{tag}");
                nodes.push(make_node(
                    "Gather",
                    vec![w_name, idx],
                    vec![gathered.clone()],
                ));

                let shape_out = format!("eshape{tag}");
                inits.push(tensor_proto_i64(
                    &shape_out,
                    &[2],
                    vec![-1, (len * embed_dim) as i64],
                ));
                let out = format!("d{tag}");
                nodes.push(make_node(
                    "Reshape",
                    vec![gathered, shape_out],
                    vec![out.clone()],
                ));
                cur = len * embed_dim;
                out
            }
        };
        prev = apply_activation(act, out, &tag.to_string(), &mut nodes);
    }

    // Stable output name.
    if prev != "output" {
        nodes.push(make_node(
            "Identity",
            vec![prev],
            vec!["output".to_string()],
        ));
    }

    let mut graph = onnx::GraphProto::new();
    graph.set_name(trained.report.model_name.clone());
    graph.set_node(nodes.into_iter().collect());
    graph.set_initializer(inits.into_iter().collect());
    graph.set_input(
        vec![value_info(
            "input",
            true,
            &[input_dim],
            onnx::TensorProto_DataType::FLOAT,
        )]
        .into_iter()
        .collect(),
    );
    graph.set_output(
        vec![value_info(
            "output",
            true,
            &[model.out_dim],
            onnx::TensorProto_DataType::FLOAT,
        )]
        .into_iter()
        .collect(),
    );

    let mut opset = onnx::OperatorSetIdProto::new();
    opset.set_domain(String::new());
    opset.set_version(OPSET);

    let mut proto = onnx::ModelProto::new();
    proto.set_ir_version(8);
    proto.set_producer_name("xazz".to_string());
    proto.set_opset_import(vec![opset].into_iter().collect());
    proto.set_graph(graph);
    Ok(proto)
}

/// Writes the trained model to `path` as an ONNX file.
pub fn export(trained: &TrainedModel, path: &str) -> Result<(), String> {
    let proto = build_model(trained)?;
    let bytes = proto.write_to_bytes().map_err(onnx_err)?;
    if let Some(parent) = std::path::Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("ONNX: checkpoint dir create failed: {e}"))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("ONNX: write '{path}' failed: {e}"))
}

/// ONNX Runtime environment (initialized once per process).
pub fn init_runtime() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = ort::init().with_name("xazz").commit();
    });
}

/// Writes the `.onnx` artifact only when it is missing or older than the
/// checkpoint it was derived from (issue D2: avoid re-exporting every predict).
pub fn ensure_export(trained: &TrainedModel, path: &str) -> Result<(), String> {
    if onnx_is_fresh(trained, path) {
        return Ok(());
    }
    export(trained, path)
}

/// Whether an existing `.onnx` is at least as new as its source checkpoint.
fn onnx_is_fresh(trained: &TrainedModel, path: &str) -> bool {
    let onnx = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let checkpoint = std::fs::metadata(&trained.report.checkpoint_path)
        .and_then(|m| m.modified())
        .ok();
    match (onnx, checkpoint) {
        (Some(onnx), Some(ckpt)) => onnx >= ckpt,
        // The `.onnx` exists but the checkpoint timestamp is unavailable → trust it.
        (Some(_), None) => true,
        _ => false,
    }
}

/// Builds an ONNX Runtime session for an exported artifact.
///
/// The execution providers come from `XAZZ_ORT_EP` (see [`execution_providers`]),
/// so a GPU-enabled ONNX Runtime build runs the graph on CUDA/TensorRT/DirectML/
/// CoreML. The session is cached by the caller, so this runs once per artifact.
pub fn load_session(path: &str) -> Result<ort::session::Session, String> {
    let eps = execution_providers()?;
    let mut builder = ort::session::Session::builder().map_err(onnx_err)?;
    if !eps.is_empty() {
        builder = builder
            .with_execution_providers(eps)
            .map_err(|e| format!("ONNX: execution provider setup failed: {e}"))?;
    }
    builder
        .commit_from_file(path)
        .map_err(|e| format!("ONNX: session load '{path}' failed: {e}"))
}

/// `XAZZ_ORT_DEVICE` device index for the CUDA/TensorRT/DirectML EPs (default 0).
fn ort_device_index() -> i32 {
    std::env::var("XAZZ_ORT_DEVICE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

/// Resolves the ONNX execution-provider list.
///
/// The unified `XAZZ_DEVICE` selector takes precedence (`auto|cpu|cuda[:N]|
/// tensorrt[:N]|directml[:N]|coreml`), with the legacy `XAZZ_ORT_EP` list as the
/// fallback. `auto` (default) registers every GPU EP compiled into this build,
/// in preference order, and degrades silently to the next provider or CPU. An
/// explicit provider is fail-closed: a requested-but-unavailable EP errors
/// instead of silently running on CPU, so a GPU benchmark cannot accidentally
/// measure the CPU provider.
fn execution_providers() -> Result<Vec<ExecutionProviderDispatch>, String> {
    if let Ok(raw) = std::env::var("XAZZ_DEVICE") {
        let spec = parse_device_spec(&raw).ok_or_else(|| {
            format!(
                "unknown XAZZ_DEVICE '{raw}' (use auto|cpu|cuda[:N]|tensorrt[:N]|directml[:N]|coreml)"
            )
        })?;
        return match spec {
            DeviceSpec::Auto => Ok(auto_providers(0)),
            DeviceSpec::Cpu => Ok(vec![build_ep(OrtEpKind::Cpu, 0)?.error_on_failure()]),
            DeviceSpec::Cuda(i) => Ok(vec![
                build_ep(OrtEpKind::Cuda, i as i32)?.error_on_failure(),
            ]),
            DeviceSpec::TensorRt(i) => Ok(vec![
                build_ep(OrtEpKind::TensorRt, i as i32)?.error_on_failure(),
            ]),
            DeviceSpec::DirectMl(i) => Ok(vec![
                build_ep(OrtEpKind::DirectML, i as i32)?.error_on_failure(),
            ]),
            DeviceSpec::CoreMl => Ok(vec![build_ep(OrtEpKind::CoreML, 0)?.error_on_failure()]),
            _ => Err(format!(
                "XAZZ_DEVICE '{raw}' is not an ONNX execution provider; \
                 use auto|cpu|cuda[:N]|tensorrt[:N]|directml[:N]|coreml"
            )),
        };
    }
    let spec = match std::env::var("XAZZ_ORT_EP") {
        Ok(raw) => parse_ort_ep_spec(&raw)?,
        Err(_) => OrtEpSpec::Auto,
    };
    let device = ort_device_index();
    match spec {
        OrtEpSpec::Auto => Ok(auto_providers(device)),
        OrtEpSpec::Explicit(kinds) => {
            let mut eps = Vec::with_capacity(kinds.len());
            for kind in kinds {
                eps.push(build_ep(kind, device)?.error_on_failure());
            }
            Ok(eps)
        }
    }
}

/// Every GPU EP compiled into this build, in preference order. Uncompiled EPs
/// are skipped (auto degrades to CPU).
fn auto_providers(device: i32) -> Vec<ExecutionProviderDispatch> {
    [
        OrtEpKind::Cuda,
        OrtEpKind::TensorRt,
        OrtEpKind::DirectML,
        OrtEpKind::CoreML,
    ]
    .into_iter()
    .filter_map(|kind| build_ep(kind, device).ok())
    .collect()
}

/// Builds one execution provider, failing closed when its `ort` feature is not
/// compiled into this binary.
fn build_ep(kind: OrtEpKind, device: i32) -> Result<ExecutionProviderDispatch, String> {
    match kind {
        OrtEpKind::Cpu => Ok(ort::ep::CPU::default().build()),
        OrtEpKind::Cuda => cuda_ep(device),
        OrtEpKind::TensorRt => tensorrt_ep(device),
        OrtEpKind::DirectML => directml_ep(device),
        OrtEpKind::CoreML => coreml_ep(),
    }
}

// Unused when every `onnx-*` EP feature is enabled (each `*_ep` helper then takes
// its compiled branch), so allow dead code rather than feature-gating the calls.
#[allow(dead_code)]
fn ep_not_compiled(kind: OrtEpKind, feature: &str) -> String {
    format!(
        "{} ({}: --features {feature})",
        tr(
            "ONNX execution provider is not compiled into this binary",
            "ONNX 실행 프로바이더가 이 바이너리에 포함되지 않았습니다"
        ),
        kind.id()
    )
}

#[cfg(feature = "onnx-cuda")]
fn cuda_ep(device: i32) -> Result<ExecutionProviderDispatch, String> {
    Ok(ort::ep::CUDA::default().with_device_id(device).build())
}

#[cfg(not(feature = "onnx-cuda"))]
fn cuda_ep(_device: i32) -> Result<ExecutionProviderDispatch, String> {
    Err(ep_not_compiled(OrtEpKind::Cuda, "onnx-cuda"))
}

#[cfg(feature = "onnx-tensorrt")]
fn tensorrt_ep(device: i32) -> Result<ExecutionProviderDispatch, String> {
    Ok(ort::ep::TensorRT::default().with_device_id(device).build())
}

#[cfg(not(feature = "onnx-tensorrt"))]
fn tensorrt_ep(_device: i32) -> Result<ExecutionProviderDispatch, String> {
    Err(ep_not_compiled(OrtEpKind::TensorRt, "onnx-tensorrt"))
}

#[cfg(feature = "onnx-directml")]
fn directml_ep(device: i32) -> Result<ExecutionProviderDispatch, String> {
    Ok(ort::ep::DirectML::default().with_device_id(device).build())
}

#[cfg(not(feature = "onnx-directml"))]
fn directml_ep(_device: i32) -> Result<ExecutionProviderDispatch, String> {
    Err(ep_not_compiled(OrtEpKind::DirectML, "onnx-directml"))
}

#[cfg(feature = "onnx-coreml")]
fn coreml_ep() -> Result<ExecutionProviderDispatch, String> {
    Ok(ort::ep::CoreML::default().build())
}

#[cfg(not(feature = "onnx-coreml"))]
fn coreml_ep() -> Result<ExecutionProviderDispatch, String> {
    Err(ep_not_compiled(OrtEpKind::CoreML, "onnx-coreml"))
}

/// Runs inference through an already-loaded session — no re-export or session
/// rebuild (issue D2). Preprocessing matches the in-memory CPU predict, so the
/// two are numerically comparable. Rows are uploaded in chunks (see
/// `XAZZ_INFER_CHUNK`) to bound peak memory on large frames.
pub fn predict_with_session(
    trained: &TrainedModel,
    session: &mut ort::session::Session,
    df: &DataFrame,
    as_col: Option<&str>,
) -> Result<DataFrame, String> {
    let (xs, n, feature_count) = super::prepare_inference_input(trained, df)?;

    let mut preds: Vec<f32> = Vec::with_capacity(n);
    for (start, end) in super::chunk_ranges(n, super::infer_chunk_size()) {
        let rows = end - start;
        let slice = xs[start * feature_count..end * feature_count].to_vec();
        let input =
            ort::value::Tensor::from_array((vec![rows as i64, feature_count as i64], slice))
                .map_err(onnx_err)?;
        let outputs = session
            .run(ort::inputs!["input" => input])
            .map_err(onnx_err)?;
        let (_shape, data) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(onnx_err)?;
        preds.extend_from_slice(data);
    }

    super::attach_prediction(trained, df, &preds, as_col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use polars::prelude::*;
    use protobuf::Message;
    use xazz_compiler::ast::{EmbeddingVocab, LayerKind, TrainConfig};

    fn train_tiny(layers: &[LayerKind]) -> TrainedModel {
        let df = df!(
            "x1" => [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0],
            "x2" => [1.0f64, 1.0, 0.0, 0.0, 1.0, 1.0],
            "y"  => [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0],
        )
        .expect("tiny dataset");
        let config = TrainConfig {
            target: "y".to_string(),
            epochs: 1,
            learning_rate: 0.05,
            batch_size: Some(3),
            validation_split: None,
            early_stopping_patience: None,
            sweep: Default::default(),
            sweep_metric: Default::default(),
            sweep_metric_explicit: false,
            sweep_sort: Default::default(),
            sweep_sort_explicit: false,
            sweep_tiebreak: Vec::new(),
            sweep_top: None,
        };
        crate::dl::train(&df, "onnx_unit", layers, &config).expect("train")
    }

    /// A Dense -> ReLU -> Dense graph exports the expected ONNX node sequence and
    /// a single input/output, parseable back as a `ModelProto`.
    #[test]
    fn dense_export_emits_gemm_relu_gemm() {
        let trained = train_tiny(&[LayerKind::Dense(4), LayerKind::ReLU, LayerKind::Dense(1)]);
        let path = "checkpoints/onnx_unit.onnx";
        export(&trained, path).expect("export");

        let bytes = std::fs::read(path).expect("read onnx");
        let mut proto = onnx::ModelProto::new();
        proto.merge_from_bytes(&bytes).expect("parse onnx");
        let op_types: Vec<&str> = proto
            .get_graph()
            .get_node()
            .iter()
            .map(|n| n.get_op_type())
            .collect();
        assert_eq!(op_types, vec!["Gemm", "Relu", "Gemm", "Identity"]);
        assert_eq!(proto.get_graph().get_input().len(), 1);
        assert_eq!(proto.get_graph().get_output().len(), 1);
        assert!(!proto.get_graph().get_initializer().is_empty());

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(&trained.report.checkpoint_path);
        let _ = std::fs::remove_file(crate::dl::manifest_path(&trained.report.checkpoint_path));
        // The shared `checkpoints/` directory is not removed: deleting it races with
        // parallel tests that are mid-save.
    }

    /// Exports `trained` and parses the artifact back into a `ModelProto`.
    fn export_proto(trained: &TrainedModel, path: &str) -> onnx::ModelProto {
        export(trained, path).expect("export");
        let bytes = std::fs::read(path).expect("read onnx");
        let mut proto = onnx::ModelProto::new();
        proto.merge_from_bytes(&bytes).expect("parse onnx");
        proto
    }

    /// Removes the ONNX artifact and the checkpoint it was derived from.
    fn cleanup(trained: &TrainedModel, path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(&trained.report.checkpoint_path);
        let _ = std::fs::remove_file(crate::dl::manifest_path(&trained.report.checkpoint_path));
    }

    /// Conv1d -> ReLU -> Dense exports Reshape → Conv (Same padding) → Reshape,
    /// with a rank-3 [out_ch, in_ch, kernel] weight initializer.
    #[test]
    fn conv1d_export_emits_reshape_conv_reshape() {
        let trained = train_tiny(&[
            LayerKind::Conv1d {
                out_channels: 2,
                kernel_size: 2,
            },
            LayerKind::ReLU,
            LayerKind::Dense(1),
        ]);
        let path = "checkpoints/onnx_conv_unit.onnx";
        let proto = export_proto(&trained, path);

        let op_types: Vec<&str> = proto
            .get_graph()
            .get_node()
            .iter()
            .map(|n| n.get_op_type())
            .collect();
        assert_eq!(
            op_types,
            vec!["Reshape", "Conv", "Reshape", "Relu", "Gemm", "Identity"]
        );

        let conv_w = proto
            .get_graph()
            .get_initializer()
            .iter()
            .find(|t| t.get_name() == "cw0")
            .expect("Conv1d weight initializer");
        assert_eq!(conv_w.get_dims(), &[2, 1, 2]);

        // Burn splits Same padding as [floor((k-1)/2), ceil((k-1)/2)].
        let conv_node = proto
            .get_graph()
            .get_node()
            .iter()
            .find(|n| n.get_op_type() == "Conv")
            .expect("Conv node");
        let pads = conv_node
            .get_attribute()
            .iter()
            .find(|a| a.get_name() == "pads")
            .expect("Conv pads attribute");
        assert_eq!(pads.get_ints(), &[1, 0]);

        cleanup(&trained, path);
    }

    /// Embedding -> Dense exports the per-column index path
    /// (Split → Clip → Add(offset) → Concat → Cast → Gather → Reshape) and a
    /// combined rank-2 [sum(vocab), embed_dim] weight initializer.
    #[test]
    fn embedding_export_emits_split_clip_gather() {
        let trained = train_tiny(&[
            LayerKind::Embedding {
                vocab: EmbeddingVocab::Shared(6),
                embed_dim: 2,
            },
            LayerKind::Dense(1),
        ]);
        let path = "checkpoints/onnx_embed_unit.onnx";
        let proto = export_proto(&trained, path);

        let op_types: Vec<&str> = proto
            .get_graph()
            .get_node()
            .iter()
            .map(|n| n.get_op_type())
            .collect();
        assert_eq!(
            op_types,
            vec![
                "Split", "Clip", "Clip", "Add", "Concat", "Cast", "Gather", "Reshape", "Gemm",
                "Identity"
            ]
        );

        // Shared vocab 6 over 2 input columns → one combined table [12, 2].
        let emb_w = proto
            .get_graph()
            .get_initializer()
            .iter()
            .find(|t| t.get_name() == "ew0")
            .expect("Embedding weight initializer");
        assert_eq!(emb_w.get_dims(), &[12, 2]);

        // Column 1 is offset past column 0's 6 rows; column 0 starts at 0.
        let offsets: Vec<f32> = proto
            .get_graph()
            .get_initializer()
            .iter()
            .filter(|t| t.get_name().starts_with("eoff0_"))
            .map(|t| t.get_float_data()[0])
            .collect();
        assert_eq!(offsets, vec![6.0]);

        cleanup(&trained, path);
    }
}
