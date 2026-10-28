# 进度

- 已读取交接和仓库规范，fetch 并核实基线；查询 AUR RPC 与官方 repo-add 文档。

- 已实现正式标签渲染，复用 AUR 编译/文件布局；共享构建入口新增 stable 模式。
- 已实现包及数据库 GPG 签名、指纹验证、对象存储别名物化、完整快照暂存后改名，不接生产发布。
- 已增加手动 CI：x86_64 正式标签构建、签名下载及小型包事务验收、复用原生安装与 GUI 启动验收。
- 已添加 docs/architecture/pacman-repository.md，记录公钥引导、升级迁移、撤源、密钥轮换、不可变发布与剩余限制。
- 本地通过：test_aur_package.py 两项测试；用户命名空间中真实 pacman 经本地 HTTP 完成测试包安装、AUR 冲突拒绝、替换、归属、升级、卸载、缺包拒绝、不可信密钥拒绝、数据库与包篡改拒绝。
- --package-dir 分支也用小型测试包独立跑通，未冒称完整应用下载验收。
- bash -n、Python 编译检查、actionlint、git diff --check 均通过。
- 未提交、未推送、未触发远端 CI、未公开发布。已向用户询问正式 HTTPS 地址和签名公钥指纹，尚无答复。

## 后续授权实施
- 已提交并推送 feat/pacman-repository：67e7186db、9cec29ab2、7175f0720、9bb92b874。
- 发布脚本增加旧库签名验证、严格版本递增、包名碰撞校验、远端上传摘要校验、旧包保留，以及首次草稿发布。
- 增加发布器的四项本地测试，均通过；actionlint 通过。没有使用生产私钥或发布公开软件源。
- 生产流水线默认关闭；仅 main 的手动 publish 可进入签名，签名依赖构建、事务与 native GUI 验收。签名使用安全 agent，无私钥导入或导出步骤。
- 已创建 GitHub pacman-production Environment，并通过 deployment branch policy 限制 main；设置 PACMAN_SIGNING_ENABLED=false。
- 已触发完整构建：https://github.com/UniClipboard/UniClipboard/actions/runs/40606677780 。截至本次记录仍排队，完整应用尚无结果。
- 已向用户询问现有 GPG 硬件密钥或专用安全签名环境；未获答复。生产发布仍需安全签名身份/runner、新稳定版本及完整 CI 通过。


## 用户授权的 Secrets 签名方案
- 用户明确同意仅针对 pacman 发布签名密钥使用 GitHub Environment Secrets 与临时内存文件系统，例外已限定写入 VISION.md / AGENTS.md。
- 本机启用了 swap，非特权 tmpfs noswap 挂载不受内核支持；改用用户/挂载命名空间内的 ramfs，已实测成功。
- 已生成两年有效的 Ed25519 正式签名密钥，指纹 4E577D740471BCE42B86AA469C10EDAECA69D90C。
- 私钥及撤销证书经 stdin 存入 pacman-production 的 PACMAN_GPG_PRIVATE_KEY / PACMAN_GPG_REVOCATION_CERT。只把公钥和指纹写入 packaging/arch/signing/。
- 已把签名作业改为一次性 GitHub 托管 runner，先 swapoff，再将私钥经 stdin 导入容器 tmpfs，不把私钥放入 Docker 环境配置。增加同路径的独立测试密钥验收作业。
- 本地内存签名验收通过：bootstrap 的秘密传输、重新导入、包/数据库签名验证和普通磁盘目录拒绝。处理了 GPG agent 异步退出导致卸载忙的问题：杀死独立 agent 后惰性卸载仅内存挂载。
- Bash 语法、Python 编译、actionlint、git diff --check 通过；尚未取得完整应用 CI 结果。
- 已提交并推送 c4f58990c（授权边界）、3a4be253e（Secrets 内存签名）、f8ce81b7d（正式身份文档）。生产签名身份变量与 Secrets 已核对，仅输出了名称与公钥指纹。
- PACMAN_SIGNING_ENABLED 已设为 true；发布仍仅 main 手动选择 publish 且全部验收通过后执行。
- 已取消过时的未开始验收 40606677780，避免阻塞同分支并发锁。当前验收为 https://github.com/UniClipboard/UniClipboard/actions/runs/40607744388 ，build 与 signing-acceptance 均排队，尚未取得 GitHub runner。
- 正式软件源未发布；仍缺完整应用/托管签名验收结果、分支集成及包含包管理器更新保护的新稳定标签。


## 完整 CI 验收完成
- 用户通知运行完成后，核实 40607744388 为 success；build、signing-acceptance、acceptance、native-install 全部通过，生产 sign/publish 按预期跳过。
- 原生安装日志的 16 项检查均 PASS：依赖安装、pacman 文件归属/完整性、GUI/daemon 动态依赖、桌面入口/图标、WebKit 命名空间、Secret Service、真实安装路径的 GUI/daemon/WebView 运行、连接文件和顶层窗口。
- 事务验收确认 AUR 冲突、安装、升级、卸载、缺包/篡改/不可信签名拒绝；完整应用包的签名下载也通过。
- 工件 pacman-stable-x86_64（约 32.7 MB）与 pacman-native-install-x86_64 均可从该运行获取。构建源是 v1.5.0，仅用于验收；不能绕过生产源更新保护门禁。
- 准备 PR：分支名符合范围；docs-site 保持现有可用 AUR 安装说明，未上线源暂不作为可用安装渠道宣传；遥测与 Rust tracing 无适用变更。
