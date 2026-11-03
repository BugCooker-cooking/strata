# Contributing to Strata

项目处于 M1b 内部开发阶段，完整检查点尚未关闭。开始前阅读[总体设计](docs/design/2026-11-02-strata-v1-design.md)、[当前计划](docs/plans/2026-11-03-workspace-and-m1b.md)、[工程与命令契约](docs/research/2026-11-m1a-format-and-commands.md)及[验收矩阵](docs/plans/2026-11-02-p0-acceptance-matrix.md)。

## 开发约定

- 安装 README 列出的工具链，运行 `make test` 和 `make lint`。
- 测试先覆盖失败路径，再实现功能；像素算子覆盖 8/16 位、透明度与 ICC。
- Rust 依赖在 workspace 统一声明，提交 `Cargo.lock`；新增 crate 时显式添加 workspace member。
- Conventional Commits：`feat:`、`fix:`、`test:`、`docs:`、`chore:`、`ci:`。署名反映实际贡献者。
- 不提交 API Key、用户图片、构建产物、生成绑定；测试素材须有可分发许可证或由测试生成。
- macOS 的 `make lint` 同时运行固定 SwiftLint 0.59.1；便携工具按 SHA-256 校验并放在 `build/tools`，不修改系统安装。首次需要下载，Linux 的原生 lint 门槛由 macOS CI 执行。

## 八条硬规则

1. R1：文档编辑走注册命令，不由 UI 直接修改。
2. R2：每条命令有说明、参数/返回值、前置条件和可执行示例。
3. R3：人和 AI 共用功能入口；权限检查仍然存在。
4. R4：AI 只走固定元工具，按需发现功能，不执行脚本/模拟点击。
5. R5：AI 接口独立于前端。
6. R6：操作权锁与资源授权在核心检查。
7. R7：文档编辑可撤销；文件写入等外部副作用不能假装随撤销恢复。
8. R8：平台 API 隔离，portable crate 在 Linux 编译测试。

新增编辑功能必须注册为带说明书的命令；保存、渲染、历史接口有变化时同步更新计划和测试。FFI 的 `open_image` 只是受信宿主便捷包装，内部仍经资源授权与 `file.open`，不能绕过执行器。

## 检查命令

| 命令 | 作用 |
|---|---|
| `make run` | 自动生成工程、构建并启动原生窗口 |
| `make test` | Rust workspace + 原生 XCTest（macOS） |
| `make test-core` | Rust workspace 测试（Linux/macOS；无 GPU 可跳过并记录） |
| `make test-gpu` | 强制真实 GPU 的渲染与 FFI 测试 |
| `make lint` | 格式检查、clippy（警告视作错误）、可移植性检查 |
| `make commands` | 命令目录元数据与隔离示例 |
| `cargo run -p strata-devtool -- demo build/new-demo` | 8/16 位 PNG/TIFF 的完整编辑、工程与导出往返 |
| `cargo fmt --all` | 格式化 Rust |

远程创建和推送由维护者确认后进行。行为准则的私密联系方式应在公开仓库前确认；不要在公开 Issue 发布敏感举报信息。
