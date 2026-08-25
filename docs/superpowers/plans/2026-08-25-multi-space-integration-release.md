# 多空间集成、安装与发布实现计划

> **面向 AI 代理的工作者：** 必需子技能：使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务实现此计划。步骤使用复选框逐项完成，发布前必须执行 verification-before-completion 和 requesting-code-review。

## 目标

把 Engine admission 修复、桌面多空间和 HarmonyOS 多空间集成为可安装版本，在真实 Windows 与手机上证明连续加入设备无需重置，并把源码、Windows 安装包和上游修复分别交付到正确仓库。

## 架构

集成按依赖方向进行：Engine 修复先发布不可变提交；HarmonyOS 内置 Engine 快照同步同一逻辑；桌面固定该 Engine 提交；两端独立构建。发布验收以真实 admission、重启恢复、跨 Space 路由和无重置连续加入为准，不能用“编译成功”替代运行时验证。

## 技术栈

Git, GitHub CLI, Cargo, Tauri, NSIS, Deveco CLI, HDC, Windows PowerShell 7.

## 交付物

- 用户 Engine fork 的修复分支与提交
- 用户桌面仓库的多空间分支与 Windows NSIS 安装包
- 用户 HarmonyOS 仓库的多空间分支与签名调试 HAP
- 向 `UniClipboard/Engine` 提交的最小上游 PR
- GitHub Release 中的 `UniClipboard_1.0.0-alpha.7_x64-setup.exe` 或版本更新后由 manifest 生成的同等 NSIS 文件名

### 任务 1：同步 Engine 修复到 HarmonyOS 快照

- [ ] 记录用户 Engine fork 的修复 SHA 和桌面 `Cargo.lock` 中解析出的 SHA，确认完全一致。
- [ ] 只把 Engine 所有的对应提交同步到 HarmonyOS `rust/space-core/`；不覆盖 HarmonyOS native 与 ArkTS 改动。
- [ ] 运行：

```powershell
./rust/verify-engine-release.ps1
cargo test --manifest-path rust/uniclipboard-native/Cargo.toml -- --nocapture
```

预期：快照一致性校验和 native 测试通过。

- [ ] 提交单一意图：`fix: sync recovered Engine admission flow`。

### 任务 2：全量静态与单元验证

- [ ] Engine 仓库运行：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- [ ] 桌面仓库运行：

```powershell
npm run lint
npx vitest run
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

- [ ] HarmonyOS 仓库运行：

```powershell
cargo fmt --manifest-path rust/uniclipboard-native/Cargo.toml -- --check
cargo test --manifest-path rust/uniclipboard-native/Cargo.toml -- --nocapture
./rust/build-native.ps1
devecocli build --modules entry@default
```

- [ ] 任何失败先归因并修复，不跳过、不把 sandbox 的 `spawn EPERM` 当代码失败；仅对同一构建命令使用批准的非沙箱重跑。

### 任务 3：构建并安装 Windows 客户端

- [ ] 在桌面仓库根运行 release 构建：

```powershell
npm run tauri build -- --bundles nsis
```

- [ ] 对生成的 NSIS 文件计算 SHA-256，确认产品名、版本、x64 架构和签名状态。
- [ ] 安装新包前关闭现有 GUI 与 daemon；保留用户当前 profile 目录，安装过程不得删除配置。
- [ ] 安装后启动 GUI，确认旧 Space 被目录迁移为第一个 profile，daemon 同时启动所有 enabled profile。

### 任务 4：构建并安装 HarmonyOS 客户端

- [ ] 运行 `./rust/build-native.ps1` 和 `devecocli build --modules entry@default`，定位签名 HAP。
- [ ] 通过 `devecocli run --module entry --product default` 安装到已连接手机；不清应用数据，先验证旧配置迁移。
- [ ] 若需要验证全新安装，再使用独立测试 profile 或经用户明确允许后清数据；不能用清数据作为正常连接步骤。

### 任务 5：执行无重置真实验收矩阵

- [ ] 场景一：当前电脑 A 创建或恢复主 Space，手机 P 加入；文本 A -> P 与 P -> A 各成功一次。
- [ ] 场景二：不重置 A 或 P，创建第二测试 Space B，让 P 加入；A 所在 Space 保持在线。
- [ ] 场景三：不重置任何设备，让第三 profile C 加入 A 的主 Space；A、P、C 成员列表一致。
- [ ] 场景四：重启桌面 daemon 与手机应用；所有 enabled Space 恢复在线，active-send 保持。
- [ ] 场景五：把 active-send 从 Space A 切到 Space B；手机本地复制只发往 B，A 不收到；来自 A、B 的入站均能显示且标明来源。
- [ ] 场景六：停止或移除测试 Space B；主 Space A 与家中电脑对应 profile 不被清空、不离线。
- [ ] 每个场景保存 daemon 与手机日志中的时间戳、profile ID、状态变更和结果；日志不得记录邀请明文、密钥或剪贴板正文。

### 任务 6：代码审查与修复闭环

- [ ] 使用 `requesting-code-review` 按 Engine、桌面、HarmonyOS 三个 diff 分别审查。
- [ ] 对每条发现执行验证、决定、修复或拒绝，并重跑受影响测试。
- [ ] 运行 `git diff --check`，确认没有生成物误入源码提交；安装包与 HAP 只进入 Release，不提交到源码树。
- [ ] 使用 `verification-before-completion` 重新运行任务 2 与任务 5 的关键命令，记录新鲜输出。

### 任务 7：推送用户仓库与发布 Windows 安装包

- [ ] 按原子意图整理提交，确认不包含 `~allenexplorer~.ala`、临时日志、设备凭据或签名私钥。
- [ ] 推送 Engine fork、桌面仓和 HarmonyOS 仓对应分支。
- [ ] 创建或更新用户 GitHub Release，上传 NSIS 安装包，资产名称与本地最终文件一致。
- [ ] 用 GitHub API 或 `gh release view` 验证资产存在、大小与 SHA-256 匹配；不能只以上传命令退出码作为成功证据。

### 任务 8：向官方 Engine 提交最小 PR

- [ ] 从官方最新 `main` 重新 rebase Engine 修复分支，只保留 Windows 原子持久化、admission 收敛和对应测试。
- [ ] PR 标题使用英文单一意图，例如 `fix: recover interrupted admissions on Windows`。
- [ ] PR 描述包含复现、根因、平台语义、测试证据和兼容性；不得包含用户设备地址、token、profile 路径或剪贴板内容。
- [ ] 推送后用 `gh pr view` 验证 base/head、提交列表和 CI 状态；若 CI 失败，定位并修复后再次验证。

