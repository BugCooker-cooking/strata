# M1b 当前 Mac 原生验收

2026-11-04。Apple M4 Mac mini / 16 GiB / macOS 27.0.1，G27M7Pro / Display P3 / scale=2，屏幕最大及 display-link nominal 120Hz，诊断显式请求 60Hz。原生视口 1664×1768 物理像素。以下结果只属于当前主机。

## 24MP × 20 独立随机图层

使用 Release 原生应用与 CADisplayLink，最终源码 `8a2abbd`。两份 6000×4000 工程均有 20 层、7680 个独立源块；8 位源 2,013,265,920 字节，16 位源 4,026,531,840 字节。固定命令轨迹，每轮 2 秒预热后连续测量 30 秒，共三轮；不操作窗口、不采样调用栈、不并行构建。遮挡、输入、最小化、屏幕/视口变化会中断。

| 位深 | 三轮 fps | 三轮 p95 帧间隔 ms | 最长间隔 ms | 结果 |
|---|---|---|---|---|
| 8 | 60.02 / 60.03 / 60.02 | 18.27 / 18.47 / 18.50 | 25.15 / 27.71 / 35.13 | 通过 |
| 16 | 60.02 / 60.03 / 60.03 | 18.20 / 18.43 / 18.39 | 34.93 / 25.23 / 34.14 | 通过 |

门槛未变：每轮 ≥58fps 且 p95≤20ms。两报告 `qualifiesAsM1HostRun=true`、无跳帧，保留全部原始时间戳与 CPU 耗时：[8 位](data/2026-11-04-m1b-24mp-20-8.json)、[16 位](data/2026-11-04-m1b-24mp-20-16.json)。最终最大间隔 35.13ms；初次缓存修复轮的 154.89ms 停顿亦保留在 `*-cache-fix.json`，不声称零卡顿。以上是成功 CPU 提交间隔，不是 GPU 完成或实际呈现时间；最终首次打开约 28/54 秒，未控制 OS 文件缓存冷度，不能称为磁盘冷启动基准。

16 位修复前同视口的三轮为 55.90 / 52.71 / 16.33fps，p95 33.90 / 38.49 / 34.44ms，最长停顿 43.96 秒；[失败原始记录](data/2026-11-04-m1b-24mp-20-16-before-fix.json)继续保留。修复覆盖 Swift 6 utility 压力回调隔离、CPU/源统一 LRU 与并发排序、合成缓存容量统计、不可变场景的指纹缓存，以及系统压力只清源/历史的策略。分配时仍按全局固定额度淘汰显示缓存。

报告的托管 CPU 峰值 2 GiB 不包含所有进程开销；物理 footprint 仍须以报告独立字段读取，不能将其称为 RSS 硬上限。

## 恢复后原生另存

启动恢复提示 → 恢复 → 原生保存面板写入新工程。A4、300 dpi、16 位、P3、白色背景与命名图层恢复；另存后为干净文档。与任务自己的崩溃前恢复工程逐块比较，元数据、140 个块、73,400,320 源字节完全一致，恢复副本剩余 0。见[机器可读证据](data/2026-11-04-m1b-native-recovery.json)。

## 24MP JPEG 可交互打开

最终实现 `8a2abbd`，当前 M4 主机三次独立新进程、无 profiler 或并行构建：1424.06 / 1390.41 / 1298.48ms，均达到 1500ms 参考值。[原始汇总](data/2026-11-04-m1b-jpeg-open.json)。OS 文件缓存冷度未控制，不能称为磁盘冷启动，也不代替 M1 MacBook Air 基准。

基线约 3252ms；优化包括单张对齐照片跳过无效混合、准确缓存 RGB8 的 256 个曲线码值、内部线性 ICC 恒等曲线避免逐样本幂函数、每线程仅保留一份小型 ICC 变换、避免不必要的全分辨率预上传和逐像素重复除法。16 位与带 LUT 的 ICC 不套用 RGB8 码值缓存；alpha 精确，线性负值/扩展值保持，混合通道与同一份序列化 ICC 的未优化 LCMS 浮点参考误差小于 1e-6。所有源样本不变。

最终多层复测曾在数码测色计运行期间失败，报告完整保留：[8 位](data/2026-11-04-m1b-24mp-20-8-meter-open.json)、[16 位](data/2026-11-04-m1b-24mp-20-16-meter-open.json)。此时 WindowServer 有持续负载；确认测色计退出后，相同源码/视口的六轮均通过。此为环境对照，未隔离所有系统后台任务，不把测色计断言为唯一原因。

## 两份原生文档的资源压力

最终源码 `8a2abbd`；同时打开独立的 8/16 位 6000×4000 工程，各 20 层/7680 独立源块，共 6,039,797,760 源字节。使用两个实际文档标签、独立引擎和 CAMetalLayer，按 1:1 遍历每个画板，显式清理后回访、重建缩略图，最后 detach 两张画布。

- 托管 CPU 峰值 2,147,483,648 字节，GPU 峰值 503,495,680 字节；均在固定默认额度内，磁盘后备峰值 6,039,797,760 字节。
- 主动 `trimMemory` 清理产生 1464 次 GPU 缓存淘汰；不是 GPU 配额耗尽测量。配额耗尽/固定读取者的错误保护另有池回归覆盖。
- 两份文档状态精确一致，每份 20 张缩略图的 122,880 字节精确一致；两个画布 detach 后托管 GPU 为 0。
- 原始报告见 [native-workspace-pressure](data/2026-11-04-m1b-native-workspace-pressure.json)。这是资源压力与重建检查，不是帧率报告或源图全样本检查；全样本保真由 50MP×30 工具单独验证。
- 此次发现诊断退出从 MainActor task 直接进入 AppKit 嵌套退出循环会等待主队列清理，报告已完成但进程须终止；已改为退出 task 后由主 RunLoop 发起退出，后续 `8a2abbd` 的完整同场景复核报告通过且进程退出码为 0。

## 窗口生命周期与引擎释放

最终应用 `c55cab6` 打开 4000×6000 JPEG，在两标签原生窗口组完成三次最小化/恢复。每次记录真实 NSWindow 状态与成功的恢复帧；活动画布缩放依次为 25.44%、20.352%、25.44%，恢复后图像完整，无错误弹窗或脏状态，正常退出码为 0。[机器可读记录](data/2026-11-04-m0-window-lifecycle.json)。每个标签都会收到窗口组回调，因此 3 轮对应 6 次 minimized 和 6 次 restored 回调，不把回调数量冒充轮次。

收尾发现全局内存压力监听强引用首个引擎，会让退休文档保留。已改为不拥有任何引擎的全局 FFI 回收入口；native XCTest 断言退休引擎释放，随后 utility 队列回调仍能保护另一引擎的状态与缩略图。完整 146 项 Rust、23 项 XCTest、lint、28 个命令示例、Release 全部通过。

## 复验入口

以下输出必须选择不存在的新路径；原生基准时保持窗口可见，关闭实时测色/屏幕采样工具，不并行编译。帧率阈值仍是每轮 ≥58fps 且 p95≤20ms。

```sh
cargo run --release --locked -p strata-devtool -- fixture-project build/new-24mp-8.strata 6000 4000 20 8
cargo run --release --locked -p strata-devtool -- fixture-project build/new-24mp-16.strata 6000 4000 20 16
make build CONFIGURATION=Release
```

Release 可执行文件：`build/DerivedData/Build/Products/Release/Strata.app/Contents/MacOS/Strata`。每个位深分别传入 `--benchmark-input`、新的 `--benchmark-output`、`--benchmark-seconds 30 --benchmark-runs 3 --benchmark-keep-visible`，用 `STRATA_SOURCE_REVISION` 记录精确提交。两文档压力使用两次 `--workspace-pressure-input` 和新的 `--workspace-pressure-output`；原生窗口回调日志用 `--window-lifecycle-log`。这些是可信宿主诊断入口，不注册为 AI 文档命令。

## 仍需完成

- P3 同屏数码测色计读数与照片对照，以及最终源码归档/CI。
- M1 MacBook Air、macOS 14、Retina/非 Retina 跨屏和真实触控板依旧未验证，按已确认的设备暂缺记录，不用当前主机或 CI 代替。
