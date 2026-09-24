# Xazz

[English](README.md) · [한국어](README_kr.md) · [日本語](README_ja.md) · [简体中文](README_zh.md)

> 翻译状态：本文是根据[英文 README](README.md) 的 `e8db438`（2026-09-24）编写的入门摘要。功能和命令的最新说明以英文版为准。更新流程见[翻译指南](docs/TRANSLATING.md)。

Xazz 是用 Rust 构建的 AI 流水线治理层。它在运行 `.xzz` 脚本之前检查类型、空值和安全策略；数据处理使用 Polars，模型训练使用 Burn。

## 快速体验

安装稳定版 Rust，在仓库根目录构建：

```bash
cargo build --release -p xazz -p xazz-runner -p xazz-exec
target/release/xazz new my-project
cd my-project
../target/release/xazz check example.xzz
../target/release/xazz run example.xzz
```

`xazz new` 生成 `data/sample.csv` 和可运行的 `example.xzz`；`xazz check` 执行静态分析；`xazz run` 使用 `xazz-runner` 和 `xazz-exec`。如需根据 CSV 推断类型定义，可执行 `xazz import data/sample.csv`。详情见[英文版快速开始](README.md#quick-start)。

## 延伸阅读

- [架构](docs/ARCHITECTURE.md)：编译、运行及进程边界
- [安全护栏](docs/SECURITY_GUARDRAIL.md)：策略和限制
- [路线图](docs/ROADMAP.md)：计划中的功能
- [贡献指南](CONTRIBUTING.md) · [许可证](LICENSE) · [发布版本](https://github.com/x1zzdev/Xazz/releases)

上述技术文档目前为英文。翻译优先级和审校方法请见[翻译指南](docs/TRANSLATING.md)。
