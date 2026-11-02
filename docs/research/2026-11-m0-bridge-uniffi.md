# M0 桥接验证（已通过本机基础验收）

## 目标与方法

Rust 1.97.1 / UniFFI 0.32.2 生成 Swift；Xcode 27.1 / Swift 6.4 使用 Swift 6 严格并发。Xcode 显式链接 Rust staticlib，生成绑定时使用 cdylib。运行 Rust 门面测试与 XCTest，并启动原生窗口。

## 结果

- Swift 调 Rust 版本/打开接口、Rust foreign-trait 回调 Swift 成功；回调通过不可变 Sendable handler 投递到 MainActor。
- 4 个 XCTest 全通过、0 跳过；示例 1024×1536、位深 8，后台打开后主线程收到事件；缺文件保留已有文档；CAMetalLayer attach/零尺寸 resize/detach 后文档保留。
- Rust 门面验证：旧 generation 不提交、回调重入状态锁无死锁、16 位源/ICC 保留、GPU 准备失败不发成功事件、不覆盖旧文档/画面。
- 未降级 Swift 语言模式；macOS 部署目标设为 14.0，但 macOS 14 实机运行仍待测。

## 遇到的问题与 M1 结论

Bash 空数组在系统 Bash 3.2 下不兼容 `set -u`，已改显式分支。所有外部回调必须在锁外；事件用队列按提交顺序投递。M1 将 bootstrap `open_image/set_document` 收进统一命令执行器，当前门面不是绕过命令的永久入口。

总体进度与其余门槛见 [M0 进度](2026-11-m0-progress.md)。
