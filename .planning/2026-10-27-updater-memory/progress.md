# 进度
- 已读取当前更新状态机、各平台宿主安装入口和 minisign 源码。
- 已确认用户授权同时修复包管理器下载策略与流式下载/安装。
- 已实现文件产物、流式下载、预哈希验签和旧签名文件映射兼容。
- AppImage、NSIS ZIP、macOS 入口已改为文件/有界流；宿主状态改为持有 Payload。
- 首次构建发现 installUpdate 遗留 u.bytes 引用，已替换。
- 正在更新生产签名器、现有 E2E 契约、pacman 前端与生成绑定。

- 256 MiB 下载、验签和 AppImage 安装回归通过：额外 Go 堆峰值 466224 字节（约 0.45 MiB）。取消、截断、错误公钥、旧签名、文件使用期间清理互斥、ZIP 提取均通过。
- 测试路径错误：从 Go 子模块执行时 mkdir 误建在 apps/gui-go/target，导致契约输出目录不存在；改为仓库根创建 target/update-contracts 后重跑。

- 六平台签名流水线通过；Windows/macOS 内部更新包交叉编译通过，React Doctor 98/100，无本次诊断。
- 原生策略测试首轮使用 release daemon，无法使用仅调试构建的开发文件密钥库，因私有 D-Bus 无系统密钥库而初始化失败；改用最新 debug daemon，保留独立 profile 与禁用剪贴板，避免触碰系统凭据。

- 原生策略测试第二轮被现有隔离守卫拒绝（/usr/bin 不是独立 uc-gui-go-* 便携目录）。改用 /opt 下私有挂载的 uc-gui-go-* 目录、真实 pacman 查询与一次性包数据库，不放宽守卫。
- Rust 测试首次过滤器没有匹配到测试，已改为 mirror_enums_share_wire_form；pacman 在 Engine 端是 Unknown 兼容适配，不能使用两端枚举的同字符串断言。

- 真实宿主策略通过（target/update-policy-native-final/result.json）：真实 pacman + 一次性包数据库识别成功；3 次 feed 检查、0 次包请求；手动下载/安装拒绝；AppImage 对照 1 次包请求，ready 后退出文件清理成功。
- 真实宿主测量（独立 Xvfb，关闭窗口后立即采样）托盘 PSS 137899 KiB；本次证实的是包大小不再导致 Go 堆增长，不能将该环境的所有宿主基础内存归于更新器。
- 流式内存回归、55 项既有安装契约、六平台签名流水线、19 个既有前端测试、Rust 枚举边界测试、Windows/macOS 模块交叉编译、宿主绑定检查及前端类型检查均通过。

- 已构建并验证 uniclipboard-1.5.0.alpha.2.local.ga46e5493-5-aarch64.pkg.tar.zst，包内三个程序哈希与 staging 一致。
- 最终独立更新窗口也使用共享安装类型路由；19 个现有前端测试继续通过、tsc 和生产前端构建通过。React Doctor 仅剩 e2e-driver.ts 中 HEAD 已有的四处 any 诊断，无新增诊断。
- go vet（gtk4,production，全部 Go 子包）通过；生产 GUI 构建通过。未执行 Windows/macOS 原生安装，只有对应更新模块交叉编译和既有安装数据契约。
- 用户当前运行的宿主尚未替换；本轮未安装系统包，也未关闭用户的应用进程。

## 本机安装后复测

- 用户安装第 5 次本地构建并重启后，可执行文件哈希与安装包一致。
- 主窗口已关闭，界面子进程已正常退出，宿主没有子进程，也没有 WebKit 渲染/网络/GPU 进程。
- 托盘 PSS 67900 KiB（约 66 MiB）、RSS 81984 KiB；daemon PSS 60318 KiB（约 59 MiB）。
- 托盘 PSS 相比修复前观察到的约 196 MiB 明显下降；该观测不等于已将全部基础开销归因于更新器。

## 提交整理

- 通知语言、桌面驻留切换、流式更新产物、包管理器策略已分为独立提交。
- 本地安装包和原生验收工件留在被忽略的 target 目录，未纳入版本控制。
