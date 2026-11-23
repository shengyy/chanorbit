# chanOrbit

轻量纯 Rust 缠论结构引擎：包含处理、分型、笔、笔中枢、MACD 区间力度、结构几何快照、已确认买侧阶段的逐 K 观察、多级别结构事实与买卖两侧区间套信号，以单一可执行文件提供，经 stdin/stdout 交换 JSON。

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

`chanorbit stages` 接受同样的请求字段，外加可选的 `observe_from`，返回最后一根 K 线时已确认的二买、三买与类二买，各带中枢、角色笔、首知时刻、阶段年龄与回试之后的状态。

`chanorbit levels` 接受同一标的由粗到细的多个周期序列，返回各级别的笔窗口、近端中枢及其相对最新价的状态、同向比较，以及次级在本级中的位置。

`chanorbit signals` 接受 2 或 3 个这样的序列，返回三买区间套（上级三买回试在次级走势完成，带阶段）、下跌末端区间套（上级下跌离开中枢并背驰，在次级走势完成）、上涨结束区间套与本级支撑；带 `observe_from` 时逐 K 重放，列出每个区间套阶段首次成立的时刻。

请求、响应、错误与退出码见 [docs/cli-contract.md](docs/cli-contract.md)；各对象定义见 [docs/structure-contract.md](docs/structure-contract.md)。

## 特性

- 命令无状态：每个请求从完整 K 线序列重算。
- 输出确定：批量输出顺序与字节不随线程数变化。
- 纯引擎：库内无网络、文件、时钟或环境变量。
- 依赖：`serde`、`serde_json`、`chrono`。

## 许可证

使用 [MIT 许可证](LICENSE)。
