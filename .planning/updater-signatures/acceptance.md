# 更新签名与 feed 验收合同

基线：origin/main 4c7cee72425655e2d7b566976ccae1485a5661b3（#1915 合并提交）；#1864 已合并。
范围：#1896。生产 daemon 保持 Rust；不改 Engine、托盘或用户 profile。

## 失败模式（实现前记录）

- CI 密钥缺失、外层 Tauri base64 格式错误、错误密码或不支持的加密参数。
- 私钥与 app.json 生产公钥不匹配；误用 SHA256SUMS 的独立密钥。
- 签名正文、可信注释或下载内容被修改；签名与文件名配错。
- 构建产物扁平化碰撞、缺少架构、同优先级重复；原始 exe/诊断 JSON 被发布。
- 缺少六个平台，Windows/Linux 安装包尚未进入生产构建。
- CI 从可变分支取得不同来源；把 fixture 或 test-mode 当正式 release。
- 注册被拒绝，或只注册 Ready 而没有渠道提升；HTTP 200 不能代表数据被接受。
- Pages 复制过时/撤回版本、覆盖其他渠道或在 primary 变化时发布不一致快照。
- 密钥/密码出现在参数、日志或工件；错误输出包含原始输入。
- 签名成功但 OS 安装、重启、启动失败。

## 状态不变量

- app.json 为唯一 updater 公钥来源；签名输出必须由 update.Client.Verify 验证后写出。
- 密钥与密码只从环境读取，CI 来源为 TAURI_SIGNING_PRIVATE_KEY 与 TAURI_SIGNING_PRIVATE_KEY_PASSWORD；禁止秘密工件。
- 单一 assemble-update-manifest.js 与 build-flare-release-registration.js 继续生成 feed/注册载荷。
- 发布前必须存在六个预期平台且只有明确命名的发布文件；禁止重命名碰撞掩盖重复。
- release.yml 的其他平台 fail-closed 保护保留。
- Pages 只复制 FlareRelease 当前已提升渠道的完整响应，保留其他站点文件；204 或已证实的空渠道 404 须删除 fallback（当前服务只有 stable/alpha）。
- 隔离 CI 不注册、不提升、不建 tag/Release、不写 R2/Pages、不上传 Sentry。

## 验收与证据边界

1. 先建真实进程 E2E：加密 throwaway key → signer → generator → HTTP feed → Client.Check/Download/Verify；记录六个平台及篡改拒绝。fixture 不算真实平台包。
2. 已有真实 macOS 归档由隔离 CI 下载；记录 upstream workflow run、head SHA、artifact 名/ID/digest；生产秘密签名并由 app.json key 验签，记录 SHA256。
3. 生产签名、真实包、真实服务注册、真实 feed 下载、OS 安装启动分别记录，不互相替代。
4. 本地 FlareRelease 验证可作为源码/协议证据，不能作为真实服务接受证明；缺 staging 时明确留下前置条件。
5. 实现 Pages 发布流程及一致性检查，但本任务不执行生产发布。
6. 保存命令、版本、当前提交、hash、stdout/stderr、失败工件。交付索引小于 50MB。
7. 未授权 merge/release/deploy；DoD 各 OS 的生产下载安装留待授权及全平台包准备完毕。
