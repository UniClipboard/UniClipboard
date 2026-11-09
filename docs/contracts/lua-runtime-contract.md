# Lua 运行时契约 v0.1

- 状态：增强 VM 与沙箱已在 `internal/luavm` 实现（指令/原生工作/字节/逻辑分配预算、粘性中止、白名单、有界编译、失败即销毁），经 `internal/luaplugin` 接入 Engine（`Options.LuaPlugins`、`ReloadLuaPlugin`）。已知与目标的差距：AST 深度/节点数以括号嵌套 ≤128 近似，分配计费只覆盖拼接/`string.rep`，表与闭包增长由指令预算间接限制，不是精确 RSS 保证；`utf8` 仅 valid/len/sub。
- 日期：2026-11-07
- 依据：[0004](../decisions/proposed/0004-clipboard-pipeline-lua.md)、[插件契约](plugin-contract.md)。

## 1. 上游验证与实现选择

验证版本固定为 `github.com/yuin/gopher-lua v1.1.2`，不以浮动 latest 作为实现依据。可复现探针见[说明](../exec/probes/lua-runtime/README.md)，未改项目 go.mod。

| 核查项 | 上游行为 | 对本项目的约束 |
|---|---|---|
| 纯 Lua 死循环 | SetContext 可在 VM 分派时检查取消 | 作为墙钟取消基础，不能冒充指令计数 |
| Go 原生函数 | 执行期间不会被 VM 取消抢占；探针中的 80ms 函数越过 10ms 截止且正常返回 | 所有开放函数须自行检查预算；宿主返回前再次检查截止时间 |
| 指令上限 | 无公开的指令计数 hook | 需要增强 VM，不能仅调用 SetContext |
| 基础库 | OpenBase 还暴露 loadstring、print、collectgarbage、getfenv/setfenv 等 | 使用逐项白名单，不能只删除三个加载函数 |
| utf8 | 默认无 utf8 库 | 提供有界的宿主工具 |
| SetMx | 检查 Go 进程整体内存，超限可 os.Exit(3) | 禁止用于插件内存限制 |

依据是上游 [VM](https://github.com/yuin/gopher-lua/blob/v1.1.2/vm.go)、[状态与 SetMx](https://github.com/yuin/gopher-lua/blob/v1.1.2/state.go)、[基础库](https://github.com/yuin/gopher-lua/blob/v1.1.2/baselib.go)、[默认库列表](https://github.com/yuin/gopher-lua/blob/v1.1.2/linit.go) 和 [README 的限制说明](https://github.com/yuin/gopher-lua/tree/v1.1.2#differences-between-lua-and-gopherlua)。探针只验证表中四项可执行行为，不验证完整沙箱或增强 VM。

选择纯 Go 增强 VM：P6 在 `internal/luavm/` 纳入固定上游版本及最小补丁，保留许可证、版本/来源及补丁说明，不新增嵌套 Go module；Engine 通过私有适配层使用，不暴露 VM 类型。补丁包含指令计数、粘滞中止、原生操作预算及有界编译/分配入口。升级须重跑本文验收，不能用升级上游版本替代补丁审查。此处确定方案，不代表已完成维护分支。

## 2. 执行与资源预算

每插件独立状态、串行回调。脚本顶层求值、Init、Start、Process、OnEvent、Close 都受限制；Lua 不创建后台任务。默认每次执行 200ms、1,000,000 条 VM 指令；宿主可收紧，首版不允许插件自行放宽。排队时间计入调用方截止时间；开始执行时取调用方和本地预算中较早的截止时间。

每条实际分派的指令扣减预算，所有 VM 循环/调用路径必须覆盖。计数耗尽、context 取消或超时后，中止标记保持到宿主最外层边界；pcall/xpcall 不能恢复预算或使失败回调提交。宿主在返回成功、提交 Item/Policy patch 前再检查预算与截止时间。启动/关闭也不得通过受保护调用吞掉中止。

原生 Go 函数的循环和分配不计作单条廉价 VM 指令。适配层对扫描字节、模式匹配步骤、表遍历和构造输出单独扣减工作预算，并在有界间隔检查取消。默认每回调最多处理 8MiB 扫描/复制字节和 1,000,000 次原生迭代；具体函数可更严，不能靠超时后遗留后台 goroutine 实现取消。未增强的上游模式匹配器不得直接开放。

资源初始边界：调用栈 128 层、注册表初始 1,024 槽/最大 8,192 槽、源码最大 256KiB、单个字符串/向 Lua 提供的文本最大 1MiB、每状态累计逻辑分配预算 16MiB。逻辑分配统计字符串字节、表槽和闭包等的约定成本，包含跨回调保留对象；首版不因 GC 或回调完成返还配额，耗尽即丢弃状态，因此长期有状态脚本也可能达到配额。临时分配同样计费，拼接、重复、格式化和表扩容必须在分配前校验，防止一次原生函数申请巨大内存。

这些是逻辑边界，**不是精确 Go 堆/RSS 上限**。注册表/调用栈限制不能限制表和字符串；不使用 SetMx，也不承诺 recover 能挽救进程 OOM。若安全验收无法在进程内约束最坏输入，P6 暂停交付，另定进程隔离方案；不能以近似计量宣称可运行任意恶意脚本。

解析/编译发生在 VM 执行前，也必须有界：源码限制之外，AST 深度最多 128、节点最多 100,000，解析/编译默认 200ms，并在解析与代码生成循环检查取消。单纯在 goroutine 外等待超时不算实现。顶层求值另取一次执行预算。超出正文限制按插件失败策略处理，不静默截断或跳过敏感处理。

## 3. 白名单与上下文

使用 SkipOpenLibs，从受审函数逐项建立环境。基础函数仅开放 assert、error、ipairs、pairs、next、pcall、xpcall、select、tonumber、tostring、type、unpack，均需遵守预算。string/table/math 也按函数白名单注册并提供有界包装，不能直接 OpenLib 整库。随机数使用实例私有状态，不暴露全进程可变状态。

不提供 os/io/debug/package/channel/coroutine、require/module、dofile/loadfile/load/loadstring、print、collectgarbage、getfenv/setfenv、newproxy、rawget/rawset、getmetatable/setmetatable。日志只经受控 ctx 接口；上下文元表不可修改，用户代码不得获得任意 Go userdata、反射或文件/网络能力。

首版 utf8 工具定义为 `utf8.valid(text)`、`utf8.len(text)` 和 `utf8.sub(text, first, last)`：len 按 Unicode 码点计数，sub 使用从 1 开始的闭区间、要求 `1 <= first <= last <= len`；无效 UTF-8 或无效区间返回调用错误。不是 grapheme 分割，也不声称兼容完整 Lua 5.3 utf8 库。扫描和输出计入预算，中文、组合字符与非法字节有独立用例。

ctx 只属于当前回调及代次，回调结束后失效；保存到全局再调用必须返回稳定错误。item/policy 是只读快照，修改只通过约定方法暂存；不存在普通 Host 再入提交或 Lua 同步后端能力。任何可返回正文的 API 都先检查大小与逻辑分配预算。

## 4. 失败与热替换

异常、中止、超限统一丢弃本次 patch，遵循插件的 continue/block_sync 策略；记录稳定错误类别，不直接转发 Lua 错误文本、源码片段或栈中的正文。取消不得在返回成功后提交迟到 patch。

失败回调可能已改变 Lua 全局状态，不能声称回滚整个解释器。首版任何执行失败均销毁该实例状态，标记不可用；不自动重放失败内容，也不对后续内容静默跳过插件。后续调度仍按其错误策略处理，直至显式重载成功；Close 在独立预算内尽力调用，失败仍释放状态。正常回调间允许保留 Lua 全局状态，但不跨重载迁移。

热替换先在独立状态完成有界加载、Init/Start，再在旧实例回调边界切换；新状态失败保留旧有效状态，成功后有界关闭旧状态。切换期间两份状态分别占用预算，不能借重载无界保留旧状态。运行失败导致的不可用与新版本加载失败保留旧版本是两种不同情形。

## 5. P6 必须通过的验收

- 指令计数覆盖两种 VM 执行循环、递归和 pcall/xpcall；死循环在顶层及所有生命周期/通知中均能终止，之后无 patch 提交。
- 恶意模式匹配、string.rep/format/gsub、表扩容、大字符串拼接、深层语法树和编译压力有界；原生调用和编译均响应取消，不留下持续运行 goroutine。
- 分配预算覆盖跨回调持久表和原生中间对象；超限丢弃状态且其他插件/Engine 继续工作，不能通过回收或反复捕获异常重置额度。
- 禁止全局、元表篡改、过期 ctx、错误文本泄漏、非法 utf8、正文超限及热替换失败均有测试；失败策略在捕获/接收/显式提交上一致。
- 记录性能基线与峰值内存，再决定预算是否需显式修订；上游四项探针通过不能替代上述验收。
