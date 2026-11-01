# GUI E2E 编写规范

本文是 Desktop GUI E2E 编写要求的统一入口。新增或修改测试时，先按本文分类，再阅读对应示例。[迁移记录](tester-army-migration.md)提供现有覆盖、构建前提和已证实限制。不要从迁移记录中的旧入口复制新的启动器。

## 1. 先确定验收目标

在修改产品代码前，写下用户操作、预期结果、可能失败的情况、目标平台和证据要求。优先使用 E2E 验证，不在实现后补写单元测试。只验证与本次改动有关的行为。

从仓库根目录查看现有入口。此命令不启动测试：

```bash
bun run e2e:gui list
```

| 验收目标 | 运行组 | 现有示例与责任 |
| --- | --- | --- |
| 页面布局、主题、键盘、菜单、纯前端交互 | `layouts` | `apps/gui-go/frontend/e2e/tester-army/settings-sidebar.e2e.mjs`；使用框架管理的页面和合成数据 |
| 中继设置等现有浏览器集成 | `browser` | `apps/gui-go/frontend/e2e/tester-army/relay-overview.e2e.mjs`；由根 `e2e.config.ts` 收集 |
| 真实 HTTP/WS、持久化、初始化或历史业务 | `backends`；现有最小链路为 `daemon` | `apps/gui-go/frontend/e2e/tester-army/history-kit.e2e.mjs`；使用隔离 daemon，不用模拟成功响应证明后端行为 |
| macOS 产品窗口、原生定位与操作 | `native-macos-history` | `apps/gui-go/e2e/tester-army-native/macos-history.e2e.mjs`；真实产品与原生可访问性驱动 |
| Windows 产品窗口、真实宿主与 IPC | `native-windows-product` | `apps/gui-go/e2e/tester-army-native/windows-product.e2e.mjs`；必须连接产品原有 WebView2 target |
| 托盘、系统快捷键、系统剪贴板、安装包，或框架尚不支持的原生行为 | 对应原生验收入口 | 先查迁移记录与 `apps/gui-go/e2e/`；保留原入口及未通过断言，不改成浏览器验收 |

`--platform` 选择浏览器皮肤，不选择宿主系统。`none` 表示无平台构建。当前隔离 `backends` 运行器要求 macOS 宿主，Windows 皮肤通过不能计为 Windows 原生通过。macOS 原生运行需要已配置的驱动和系统权限。普通 CI 的收集检查只证明测试可发现。

## 2. 复用夹具和断言

1. 阅读所选组的现有示例和配置，检查当前固定依赖提供的 API。不要为同一能力新增包或自行实现驱动。
2. 布局测试从 `tester-army/page.mjs` 导入 `test` 和 `outputFor`。需要额外上下文时使用 `createContext` 夹具，由夹具关闭上下文。
3. 后端测试复用 `tester-army/backend-page.mjs`、现有会话安装逻辑和私有 daemon 夹具。不要连接日常使用的 daemon。
4. 新测试优先使用 SDK 的语义定位器。已有 DOM 几何断言可以通过官方 `surfaceOf()` 接口使用框架管理的页面。底层页面操作不等于 SDK 逐步事件，报告必须分别保留实际断言和截图。
5. 浏览器和原生套件需要同一业务场景时，复用 `e2e/scenarios/` 中的断言所有者。驱动层负责启动、会话、页面与资源清理，不复制业务断言。
6. 断言应验证用户可观察结果。等待实际就绪状态，不用固定延时或自动重试掩盖失败。主题相关行为覆盖明暗主题。

移植旧场景时，保留断言清单和原始失败工件。不要删除步骤、扩大视口或放宽阈值制造通过。产品契约确已变化时，记录契约依据、旧断言与新断言的差异，并重新实际运行。

## 3. 注册测试

布局与后端矩阵的唯一来源是 `apps/gui-go/frontend/e2e/tester-army/catalog.mjs`。不要向选择模块 `suites.mjs`、`backend-suites.mjs` 添加平行矩阵。不要另建矩阵调度器或逐套件启动脚本。

### 布局测试

1. 添加 `tester-army/<suite>.e2e.mjs`。
2. 在 `catalog.mjs` 的 `suites` 中声明夹具、支持皮肤及必要的视口参数。复用已有夹具；确有需要时在 `e2e/fixtures/` 添加使用真实组件的夹具。
3. 先以显式套件名运行候选。`e2e.layouts.config.ts` 按套件名收集测试，服务按目录中的夹具名构建。文件名和目录项必须一致。
4. 实际通过后，才把对应皮肤加入 `verifiedPlatforms`。未通过的候选写入 `retainedReasons`，不能加入默认通过矩阵。

### 真实后端测试

1. 添加 `tester-army/<suite>.e2e.mjs`，复用后端夹具。
2. 在 `catalog.mjs` 的 `backendSuites` 中声明支持皮肤和数据集。按需扩展既有数据准备逻辑，保持隔离与清理所有权。
3. 先显式运行候选。只有完整目标阶段实际通过后，才把准确的套件、皮肤、数据集和阶段加入 `verifiedBackendRuns`。失败范围在 `retainedBackendReasons` 中说明。

普通 `browser` 和原生组由各自根配置的 `tests` 字段收集，不由布局矩阵自动发现。修改这些组时，检查对应配置和统一运行器的现有分组。只有现有分组无法表达目标时才扩展入口；不得绕过统一运行器的资源清理。

`migration.json` 只记录已有入口的迁移与退役，不是新测试的发现机制。迁移原入口时同步更新；新增独立场景不应伪造旧入口记录。

## 4. 隔离与安全

- 只使用明确的合成数据、私有 HOME、唯一 profile 和专属临时目录。复用夹具已有的系统剪贴板禁用和开发凭据隔离设置，不扩大既有安全例外。
- 服务只监听 loopback，使用框架分配端口。不要使用用户正在运行的浏览器、产品窗口或 WebView 调试端口。
- 由统一运行器持有构建目录。只清理本次拥有的进程和目录。确认进程退出后再删除；无法确认时保留沙箱并报告清理失败。
- 不把口令、令牌、连接文件、私有数据库、用户内容或原始后端日志放入公开工件。启动诊断使用现有脱敏字段白名单。
- 正常退出与强制终止分别记录。强制终止不能作为正常清理通过。

## 5. 实际运行并保存工件

先完成迁移记录中的依赖安装和构建前提。下列命令均从仓库根目录执行；后端与原生命令必须在对应宿主执行：

```bash
bun run e2e:gui layouts settings-sidebar --platform macos
bun run e2e:gui backends history-kit --platform windows --seed history
bun run e2e:gui native-macos-history
bun run e2e:gui native-windows-product
```

以上是现有套件示例，按实际目标选择一条。新增套件使用自己注册的名称。布局 `--out` 只接受一个显式套件和全新导出目录。不要覆盖旧失败证据。

浏览器或后端运行后，执行公开工件汇总：

```bash
python3 apps/gui-go/frontend/e2e/tester-army/receipt.py
```

检查汇总中当前选择没有缺失报告。旧 HEAD 的报告和未执行目录不能作为本轮通过证据。SDK 的可选源码字段缺失时如实记录，结合独立 Git 回执、源码哈希和构建回执核对，不补造字段。

| 组 | 标准工件目录 |
| --- | --- |
| 布局 | `target/tester-army-layout/<皮肤>/<套件>/` 与 `target/tester-army-layout-report/` |
| 后端 | `target/tester-army-backend/<皮肤>/<套件>/<数据>/<阶段>/` 与 `target/tester-army-backend-report/` |
| 普通浏览器、最小 daemon | `target/tester-army/`、`target/tester-army-daemon/` |
| macOS 产品原生 | `target/tester-army-macos-history/` |
| Windows 产品原生 | `target/tester-army-windows-product/` |

每次交付必须提供可复跑命令、宿主与运行时、源码 HEAD 与脏状态、运行编号、断言结果、截图或场景 JSON、清理回执和工件校验和。原生验收还须核对实际产品目标及构建身份。选择回执、框架报告和截图共同证明本轮范围，任意一项不能单独代表全部验收。

## 6. CI 与交付

1. 检查 `.github/workflows/tester-army-e2e.yml` 和 `tester-army-native.yml` 的路径过滤、测试收集和工件上传。新增文件位于目录内不一定代表新增测试已运行。
2. 先通过显式候选，再通过加入后的默认矩阵。保留实际执行证据。修改源码后重新核对精确 HEAD，旧版本通过不能证明新版本通过。
3. 报告分别列出实际通过、实际失败和未运行范围。编译、收集成功、能力探针成功和 CI 跳过均不能写为产品原生验收通过。
4. 现有失败、未知契约和不支持的平台继续保留入口。记录限制和后续处理，不静默删除测试。
