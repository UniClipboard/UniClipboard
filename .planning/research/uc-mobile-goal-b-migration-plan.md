# 目标 B 方案：mobile-sync 共享逻辑零回归迁移（uc-ios → Rust）

> 前置：spike B0–B2 已完成（FFI 管道证明成立，见 `uc-mobile-spike-plan.md`）。
> 输入：`uc-ios-feature-inventory.md`（行为基线）+ `uc-ios-regression-checklist.md`（验收闸门）。
> 范围拍板（2026-06-12）：**只做 mobile-sync，不做 P2P**——本方案不含任何 Transport/iroh/加密栈内容。
> 状态：提案，待审。语言审查豁免路径（`.planning/`）。

## 0. 一句话定位

把 uc-ios `Shared/`（Network/Models/Cache）里「给定输入 → 确定输出」的纯逻辑迁入共享 Rust crate，iOS（未来 Android）经 UniFFI 调用，**验收标准 = 回归清单全绿**。UI、剪贴板 I/O、SSID 探测、扩展壳全部留原生。

## 1. 目标拓扑（在 spike 产物上生长，不另起炉灶）

```
crates/
├── uc-mobile-proto    ← 纯编解码叶子（现有 connect_uri + 本方案 M1 扩容）
│     新增：wire 模型(Clipboard/HistoryRecord)、sha256 大写 hex、
│     长文本溢出、multipart builder、ISO-8601、URL 分类/SSID 归一/排序
│     deps 只准加：sha2、unicode-segmentation（字素计数）—— 仍零内部依赖
│
└── uc-mobile          ← FFI 边界（现有 client.rs 扩容）
      M2+：完整 HTTP 客户端(A6)、ConnectionTester(A7)、
      SettingsStore/watermark/loop-guard 逻辑、SyncEngine 决策核
      I/O 一律 snapshot 经 PlatformBridge，不在 async 内回调原生
```

**单一真相收敛（顺手但单列 commit）**：daemon 的 `sync_doc.rs::SyncClipboardDoc`（server 侧）改为依赖 `uc-mobile-proto` 的规范 wire 类型，消除 Rust 侧两份 serde 定义（spike 期间 uc-mobile 里的 `WireDoc` mirror 同时收敛）。TS / 旧 iOS 实现的漂移仍靠 golden vector 锁。

## 2. Oracle 策略（先于一切端口工作敲定）

字节兼容是 #1 回归风险，而 **真实 daemon 对一半端口是假 oracle**（history query/PATCH 是兼容壳：patch 不读 body、version 硬编码 0、无 409、无 modifiedAfter，`routes.rs:15-16`）。按端口分三类：

| 端口 | oracle | 手段 |
|---|---|---|
| SyncClipboard.json get/put、file get/put、Basic Auth | ✅ 真实 daemon | 🔗 e2e（B2 编排脚本 `run-b2-daemon-demo.sh` 直接复用扩展） |
| connect-uri、hash、长文本溢出、multipart、HistoryRecord 编码 | ✅ iOS 现有 golden vector / 单测 | 🧬 把 uc-ios 仓库的测试向量 **原样移植** 成 Rust 测试（M0），iOS 实现是规范源 |
| history version/409、PATCH `isDelete`、modifiedAfter | ⚠️ 二者皆不可靠 | 从 iOS 真机/官方 SyncClipboard server 抓字节 fixture 入库（`crates/uc-mobile-proto/tests/fixtures/`）；daemon 兼容壳修复另开 issue，不阻塞本迁移 |

## 3. 里程碑（每个独立可验收、可暂停）

### M0 · 契约先行：golden vector 全量移植（小，先做）
把 checklist A 区的跨语言向量移植成 Rust 测试（红的允许存在，作为后续里程碑的驱动）：connect-uri（已有 ✅）、Clipboard JSON nil 省略、composite/split id、sha256 大写、10240 字素溢出、multipart CRLF/quoted、ISO-8601 四种组合、Basic Auth。
**验收**：A 区每条 🧬 都有对应 Rust 测试（可暂 `#[ignore]`），fixture 来源注明。

### M1 · uc-mobile-proto 扩容：纯编解码全集（中）
A2/A3/A4/A5 + B 区纯逻辑：wire 模型、hash、长文本溢出（**字素计数用 unicode-segmentation，非字节非 code point**）、multipart builder、TypeMask、ISO-8601、URL 分类（LAN/TS/WAN 网段表）、SSID 归一、Layer-1 形态排序、try-order、`isDelete`/`isDeleted` 封装 helper（裸字符串不准出现在调用点）。
**验收**：M0 测试全绿（去 `#[ignore]`）；daemon `sync_doc.rs` 改依赖 proto 类型且桌面测试无回归。

### M2 · uc-mobile HTTP 客户端补全（中）
在 B2 `client.rs` 基础上补 A6 全集：history query（multipart POST）/history data 端点、base-url 归一、文件名前置校验、状态映射表（200/201/204、401、404、5xx、其余 4xx）、重试语义（仅首遇 connection-lost/timeout，300ms 一次，401/404 不重试）、`cancel_in_flight` 后续请求立抛 cancelled。
**验收**：A6 全条（mock 单测 + 真实 daemon e2e 跑 doc/file 端点）；缝 3 drop 测试扩展到新端点。

### M3 · ConnectionTester（小）
A7：单 URL test、多 URL 并发 probe（2s 超时、404/401=可达）、`firstReachable` 按序确定性取首达（非竞速）。网络 epoch 由原生传入快照参数，Rust 不订阅系统事件。
**验收**：A7 + B 区 `preferredURLs` 全条单测绿。

### M4 · 状态与持久化逻辑（中）
SettingsStore 键名/默认值/前向兼容（E 区）、watermark、history 去重 append（cap 200、direction 升级）、SyncLoopGuard 状态机、PayloadCache 的 LRU 索引决策（驱逐 **决策** 在 Rust，文件读写/原子写由原生按决策执行——snapshot in、command out）。
**验收**：E/F 区 🔬 条目全绿；损坏 blob 返默认不阻塞启动。

### M5 · SyncEngine 决策核（大，最后做，先拆层再迁）
968 行状态机不整体搬。拆两层：
- **决策核（进 Rust）**：纯函数 `fn decide(tick_input) -> Vec<SyncAction>`——server-wins 排序、去重三守卫、push 前提判断、loop-guard 计数、退避计算。输入是原生收集的快照（剪贴板 hash、changeCount、网络上下文、settings），输出是动作列表（fetch/apply/push/throttle）。
- **执行壳（留原生）**：tick 调度（1Hz/5s/暂停）、scenePhase、UIPasteboard 读写、banner。
**验收**：C 区 🔬 条目以决策核单测覆盖；🔗 条目过 daemon e2e；📱 条目留 M6。

### M6 · uc-ios 接入与灰度（跨 repo）
xcframework 经 SPM binaryTarget 进 uc-ios；**feature flag 双路径**（原生/Rust 各保完整路径，A/B 定位回归来源——checklist 执行建议 #3）；按 M1→M5 的顺序逐模块切换，每切一个模块过一遍对应 📱 清单；三进程上下文 TLS 验收（spike 遗留）在此补。全绿后删原生路径（不留无限期双实现）。
**验收**：回归清单逐条附验证者/日期/证据；双路径删除 PR 合并。

### 持续项（不单列里程碑）
- CI：交叉编译 + bindgen drift 检查 + 体积预算 + aws-lc-rs 断言（脚本已有，搬进 workflow）。
- uniffi/toolchain 版本钉死不变（=0.31.1 / 1.95.0），升级单独评估。

## 4. 明确不做

- P2P / Transport 抽象 / iroh / 加密栈（mobile-sync 是明文 HTTP + Basic Auth，引入加密栈只会拖重 crate）
- 键盘/分享/Intents 的 UI 壳与系统钩子、剪贴板 I/O、SSID 平台 API（永留原生）
- daemon history 兼容壳的功能补全（version/409/modifiedAfter）——另开 issue，是服务端工程不是迁移工程
- Android 客户端实装（crate 按 iOS+Android 共享设计，但 Kotlin binding 与 Android 接入不在本方案）

## 5. 风险与对策

| 风险 | 对策 |
|---|---|
| 字素 vs 字节 vs code point（10240 阈值） | unicode-segmentation + 专门 golden vector（emoji/组合字符用例） |
| `isDelete`/`isDeleted` 写错 | proto 层 helper 封装，clippy 禁裸字符串（grep CI 检查） |
| 假 oracle 端口拿不到 fixture | M0 阶段就抓真机字节；抓不到的端口降级为「iOS 单测向量为准」并在清单上标注 |
| SyncEngine 拆层后行为漂移 | 决策核输入输出全部可序列化，原生侧录制真实 tick 快照回放进 Rust 测试 |
| uc-ios 双路径维护拖长 | 每个模块切换后两周内删原生路径；删除是 M6 验收项不是可选项 |

## 6. 与现有文档的关系

- 验收唯一标准：`uc-ios-regression-checklist.md`（本方案的里程碑↔清单分区映射：M0/M1↔A、M3↔A7+B、M4↔E/F、M5↔C、M6↔D/G–L）。
- 行为语义查询：`uc-ios-feature-inventory.md`。
- 管道与执行模型（runtime/缝 1/2/3）：`uc-mobile-spike-plan.md`，本方案不重复。
