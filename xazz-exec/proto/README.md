# ONNX schema

`onnx.proto` and `LICENSE` are copied unchanged from ONNX v1.16.2:
https://github.com/onnx/onnx/tree/v1.16.2

The optional `onnx` feature generates Rust types using protobuf-codegen 3.7.2's
pure Rust parser. The runtime is pinned to the same patched version. No external
protoc, network access at code generation time, or protobuf 1.x is required.

The exported model remains IR version 8 / opset 13; using a newer schema does
not change those compatibility contracts. Keep the upstream Apache-2.0 license.
