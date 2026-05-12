# A1 Findings — uc-application + uc-core

**Scope**: `src-tauri/crates/uc-application/` (10137 行变更) + `src-tauri/crates/uc-core/` (2077 行变更)
**Base**: main → HEAD `ea09cdd3`

## 🔴 必删 (死代码 / 漏调)

### `SearchFacade::clear_coordinator()` 无 production caller

- **位置**: `src-tauri/crates/uc-application/src/facade/search/mod.rs:118-122`
- **现象**: doc 写"daemon 退出时 caller 调", 但 `uc-desktop/src/daemon/host.rs:255-258` 的 daemon cleanup 路径只调 `clear_daemon_lifecycle`, **漏调** `search.clear_coordinator`。`grep -rn "clear_coordinator\b" src-tauri/` 全仓只命中定义点和自身 doc, production caller = 0。
- **建议**: 二选一 — (a) 若 search coordinator 内部 task 绑 daemon-lifecycle, 在 host.rs L255-258 补上调用 (真 bug); (b) 若实际能跨 daemon reload 复用，删除该方法。鉴于 search_assembly 是 build_daemon_search_assembly 产物，大概率属 (a)。

### `DesktopRuntime::set_event_emitter` + `emitter_cell` RwLock 链 — swap 路径无 caller

- **位置**: `uc-desktop/src/runtime.rs:129-144`、`uc-tauri/src/bootstrap/runtime.rs:149-151`, 以及一路向上透传的 `uc-bootstrap/src/assembly.rs:158,965-966`、`uc-bootstrap/src/space_setup.rs:138`、`uc-bootstrap/src/file_transfer_lifecycle.rs:85,103`、`uc-application/src/facade/host_event/publisher.rs:15,22,27,51`、`uc-application/src/facade/blob_transfer/facade.rs:27,492,512`
- **现象**: `Arc<RwLock<Arc<dyn HostEventEmitterPort>>>` 类型层层包裹，本意是"GUI shell 起步 logging, daemon 起来后 swap 真 emitter"。但 `grep -rn "\.set_event_emitter("` 全仓无外部 caller —— `run.rs:135` 和 `host.rs:84` 都构造 `LoggingHostEventEmitter`, daemon 路径用的是另一条 `event_tx` (MPSC), emitter_cell 实际生命周期是只读单值。
- **建议**: 把 emitter_cell 类型简化为 `Arc<dyn HostEventEmitterPort>`, 删除 `set_event_emitter`。属跨 crate 改造 (uc-application + uc-bootstrap + uc-desktop + uc-tauri 都要动), 建议独立 phase 推进，A2/A3 一并审。

## 🟡 可削减 (机制合理但当前规模过大)

### `AppFacade` 上 5 个 `Arc<ArcSwapOption<XxxFacade>>` 字段 — 方案 C 后真实 swap 频率 = 1

- **位置**: `uc-application/src/facade/app_facade.rs:75-105,150-168` (5 字段 + swap/clear API), 以及方法 L176-447 的 ~20 处 `.load_full()` 包装。
- **现象**: `swap_daemon_lifecycle` / `clear_daemon_lifecycle` 全仓 **只在 `uc-desktop/src/daemon/host.rs:216,257` 各一处调用**。方案 C (`0f4fa652`) 后 in-process daemon reload 取消，一个进程生命周期内 swap 1 次 + clear 1 次，且 clear 紧跟进程退出。无锁切换的性能优势在这种调用频率下完全用不上。
- **代价 vs 收益**: 改回 `Option<Arc<X>>` 要回退 ~20 处 `.load_full()` → 当前每个 thin method 多一行解包; ArcSwap 的额外认知负担也只是这 20 处。
- **建议**: 不立即拆。中期可考虑改为 `tokio::sync::OnceCell<Arc<XxxFacade>>`, 语义上更贴"启动期一次性装入", 省掉 `clear_daemon_lifecycle`。**这是方案 C 之后唯一一处"机制设计大于实际所需"的明确信号**, 但属"可削减但不必紧急删"。

### `MobileSyncFacade` 986 行 / 11 个 use case

- **位置**: `uc-application/src/facade/mobile_sync/facade.rs`
- **现象**: facade 持 11 个 use case, 每个方法都是 thin pass-through, 严格 §11.2; 但单文件偏大。
- **建议**: 不动。规模看似大，内部职责单一 (都是 iOS Shortcut LAN 出口), 拆分会让 AppFacade 字段数膨胀。

## 🟢 待定

### `MobileSyncFacadeDeps.apply_inbound: Arc<ApplyInboundClipboardUseCase>` — 裸 UseCase 跨 facade 共享

- **位置**: `uc-application/src/facade/mobile_sync/facade.rs:125` (deps 字段) + `uc-desktop/src/daemon/host.rs:213` (bootstrap 透传)
- **现象**: `apply_inbound` 由 bootstrap 装一份后，同时喂给 `MobileSyncFacade` 与 `InboundClipboardFacade`。这与 §11.4.4 "bootstrap 只持 Facade 不持 UseCase" 的纪律存在张力。
- **需要确认**: 是否应抽一个共享小 facade, 两处都走 facade 调用？还是 UseCase 是 stateless, Arc 共享语义上等价于共享 trait 可接受？

## 结论

uc-application + uc-core 这次 diff **没有大规模过度设计**。新增的 mobile_sync domain (uc-core) + facade + 7 个 mobile_sync ports + connection_channel port 都符合 hex arch — 每个 port 都 1 production adapter (uc-infra) + N test fake, 与 AGENTS.md §5.2/§11.4 一致。

**方案 C 之后唯一真正失去驱动力的抽象** 是 `AppFacade` 的 5 个 `ArcSwapOption` 字段 (原为高频 reload 设计，现在 swap 频率 = 1)。但回退 `.load_full()` ~20 处的成本与当前架构复杂度大致相当，**未达"必删"**。

真正应立即处置的是两个具体死路径：

1. `SearchFacade::clear_coordinator` 无 caller (漏调或死代码，二选一确认)
2. `emitter_cell` RwLock 包裹永远不 swap (跨 crate 假灵活，可一次性简化为 `Arc<dyn …>`)

uc-core 的 diff 全部是干净的新增 domain + port, 无冗余。
