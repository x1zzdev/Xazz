# Xazz

[English](README.md) · [한국어](README_kr.md) · [日本語](README_ja.md) · [简体中文](README_zh.md)

> 翻訳ステータス: [英語版 README](README.md) の `e8db438` (2026-09-24) を基にした入門用の要約です。機能やコマンドの最新情報は英語版を確認してください。更新方法は[翻訳ガイド](docs/TRANSLATING.md)にあります。

Xazz は Rust で実装された AI パイプライン向けのガバナンス層です。`.xzz` スクリプトを実行前に解析し、型・null 値・セキュリティポリシーを確認します。データ処理には Polars、学習には Burn を使用します。

## 試してみる

Rust stable を用意し、リポジトリのルートでビルドします。

```bash
cargo build --release -p xazz -p xazz-runner -p xazz-exec
target/release/xazz new my-project
cd my-project
../target/release/xazz check example.xzz
../target/release/xazz run example.xzz
```

`xazz new` は `data/sample.csv` と実行可能な `example.xzz` を作成します。`xazz check` は静的解析を実行します。`xazz run` は `xazz-runner` と `xazz-exec` を使用します。CSV から型を推定するには `xazz import data/sample.csv` を使用します。詳細は[英語版の Quick Start](README.md#quick-start) を参照してください。

## さらに読む

- [アーキテクチャ](docs/ARCHITECTURE.md) — コンパイル、実行、プロセス境界
- [セキュリティガードレール](docs/SECURITY_GUARDRAIL.md) — ポリシーと制約
- [ロードマップ](docs/ROADMAP.md) — 計画中の機能
- [コントリビューション](CONTRIBUTING.md) · [ライセンス](LICENSE) · [リリース](https://github.com/x1zzdev/Xazz/releases)

リンク先の技術文書は現在英語です。翻訳の優先順位とレビュー方法は[翻訳ガイド](docs/TRANSLATING.md)にまとめています。
