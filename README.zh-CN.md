# chanOrbit

轻量纯 Rust 缠论结构引擎：包含处理、分型、笔、笔中枢、MACD 区间力度、结构几何快照与已确认买侧阶段的逐 K 观察，以单一可执行文件提供，经 stdin/stdout 交换 JSON。

[English](README.md) · [产品范围](PRODUCT.md) · [文档](docs/README.md) · [测试状态](docs/status.md)

> **仅供研究与实验，非商业用途。** 不构成投资建议或交易信号，按现状提供。详见 [PRODUCT.md](PRODUCT.md#intended-use-and-disclaimer)。

## 构建

```sh
cargo build --release      # target/release/chanorbit
```

目标平台与静态 Linux 构建见 [docs/release.md](docs/release.md)。

## 使用

```sh
echo '{"id":"demo","dt":["2026-03-02T15:00:00","2026-03-03T15:00:00"],
  "high":[10.3,10.4],"low":[9.9,10.0],"close":[10.2,10.1],"vol":[1000,1200]}' | target/release/chanorbit geometry
```

响应即几何快照：带区间力度的笔、近端分型、规范中枢序列及最近两个中枢、候选段及其力度、MACD 柱与分段。批量模式使用 JSON Lines：

```sh
target/release/chanorbit geometry --batch < requests.jsonl > responses.jsonl
```

`chanorbit stages` 接受同样的请求，返回最后一根 K 线时已确认的二买、三买与类二买，各带中枢、角色笔、首知时刻、阶段年龄与回试之后的状态。

请求、响应、错误与退出码见 [docs/cli-contract.md](docs/cli-contract.md)；各对象定义见 [docs/structure-contract.md](docs/structure-contract.md)。

## 特性

- 命令无状态：每个请求从完整 K 线序列重算。
- 输出确定：批量输出顺序与字节不随线程数变化。
- 纯引擎：库内无网络、文件、时钟或环境变量。
- 依赖：`serde`、`serde_json`、`chrono`。
