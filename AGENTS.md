# chanOrbit · Agent 指引

纯 Rust 缠论结构引擎：包含处理、分型、笔、笔中枢、MACD 区间力度、未成笔候选段、结构几何快照与买侧阶段（二买、三买、类二买）的逐 K 观察；产物是单一可执行文件 `chanorbit`（stdin/stdout JSON）。本文件只放每次任务都成立的边界与路由，细则按下表加载。

## 按需加载

| 范围 | 先读 |
|---|---|
| 产品范围、非目标与用途声明 | [`PRODUCT.md`](PRODUCT.md) |
| 文档与目录职责、写入规则与上下文分层 | [`docs/README.md`](docs/README.md) |
| 模块地图与数据流 | [`docs/architecture.md`](docs/architecture.md) |
| 输出对象、字段与结构定义（包含、分型、笔、撤笔、中枢、力度、候选、阶段、观察） | [`docs/structure-contract.md`](docs/structure-contract.md) |
| 命令行、请求/响应 JSON、错误与退出码 | [`docs/cli-contract.md`](docs/cli-contract.md) |
| 测试覆盖与未覆盖项 | [`docs/status.md`](docs/status.md) |
| 构建与目标平台 | [`docs/release.md`](docs/release.md) |
| 门禁命令与提交流程 | [`CONTRIBUTING.md`](CONTRIBUTING.md) |
| 信任边界 | [`SECURITY.md`](SECURITY.md) |

## 边界

- 引擎库无 I/O、无时钟、无环境变量；进程 I/O 只在 `src/main.rs`。依赖限于 `serde`、`serde_json`、`chrono`（测试另用 `flate2`），不引入大依赖。
- 结构定义以 `docs/structure-contract.md` 为唯一 owner；改变任何成笔、中枢、力度、候选、阶段或观察行为都会使 `tests/golden/` 回归基线失败，须先由用户决定是否修订定义，不得为通过测试改基线或放宽容差。
- 输出合同变更（删改字段或语义）须升级对应命令的 schema（`contract::SCHEMA` / `contract::STAGES_SCHEMA`）并同步 `docs/cli-contract.md`、`docs/structure-contract.md`。输出只放有真实消费者的字段，每个事实只出现一次，不为旧接口保留别名或切片参数。
- 命令无状态：每次从完整 K 线序列重算；库内保留逐 K `update` 作为唯一计算路径，批量构造只是同一循环。
- 按将来可能公开处理：不提交真实行情、账号、凭据、个人绝对路径；夹具只用合成序列。
- 一个事实一个 owner；替代实现落地时同步删除旧路径、旧测试与旧文档。
- 单文件 ≤800 行；800–1200 行按职责考虑拆分；不用转发模块或 part1/part2 凑行数。新目录登记到 `docs/README.md`。

## 协作

- 中文交流，标识符、命令、路径保持英文；公共文档默认英文，两份 README 同步。
- 开工确认 cwd、分支与已有 diff，保留无关改动。
- 门禁命令由 `CONTRIBUTING.md` 持有；行为变化记入 `CHANGELOG.md` 的 `[Unreleased]`。
- 许可证为 MIT（[`LICENSE`](LICENSE)）；建远端与推送由用户决定。
