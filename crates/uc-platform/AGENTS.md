# `uc-platform/AGENTS.md`

## 1. 文档目的

`uc-platform` 是 UniClipboard 的平台适配层，负责承接不同操作系统、运行环境与宿主平台之间的差异，并向上层提供稳定、受控的平台能力。

本规范用于约束所有开发者和 AI Agent 在修改 `uc-platform` 时的设计与编码行为，确保：

* 平台差异被集中管理，而不是向上层扩散
* 操作系统特有能力有明确边界
* 不把平台细节泄漏到 `uc-core` 和 `uc-app`
* 不把 `uc-platform` 做成“什么系统相关都往里放”的垃圾层
* 条件编译、路径、权限、进程、系统集成等能力可维护、可替换、可测试

**任何修改 `uc-platform` 的提交，都必须遵循本规范并完成自我审查。**

---

## 2. `uc-platform` 的定位

### 2.1 核心职责

`uc-platform` 只负责以下事情：

1. **封装操作系统差异**

   * Windows / macOS / Linux 行为差异
   * 桌面端 / 移动端 / CLI 宿主差异
   * 权限、路径、系统服务模型差异

2. **提供平台能力适配**

   * 应用目录定位
   * 系统通知
   * 托盘
   * 开机启动
   * 系统 credential store / keychain 入口
   * 平台级 clipboard / file association / shell integration
   * 进程实例锁、单实例检查、socket/path 选择等

3. **收口条件编译**

   * 将 `cfg(target_os = ...)` 收敛在平台层
   * 不让平台分支散落到 app/core/上层表示层

4. **向上层暴露稳定的平台语义**

   * 例如“应用数据目录”“用户缓存目录”“是否支持后台常驻”“当前平台能力集”

5. **承接运行环境差异**

   * desktop / mobile / headless / daemon-only 模式
   * 不同宿主对能力的支持范围

---

### 2.2 非职责

以下内容**不属于** `uc-platform`：

| 类别        | 示例                                                       |
| --------- | -------------------------------------------------------- |
| 领域模型      | `ClipboardEntry`、`Space`、`Device`                        |
| 应用流程      | setup 状态机、join flow、facade                               |
| 基础设施实现主逻辑 | sqlite repository、libp2p network adapter、blob repository |
| UI 页面逻辑   | 视图状态、前端交互                                                |
| 启动装配总协调   | 全局 wiring、依赖注入主入口                                        |
| 表示层协议     | HTTP DTO、Tauri command payload、CLI 参数结构                  |

---

## 3. 分层关系

可以这样理解：

```text
UI / CLI / Daemon / Mobile Host
          ↓
        uc-app
          ↓
       uc-core
          ↑
       uc-infra
          ↑
     uc-platform
```

更准确地说：

* `uc-platform` 是 **平台差异与宿主能力层**
* `uc-infra` 是 **外部能力实现层**
* 两者可能协作，但职责不同

### 强制规则

* `uc-platform` 可以依赖 `uc-core`
* `uc-platform` 可以被 `uc-infra`、`uc-app`、`uc-bootstrap` 使用
* `uc-platform` 不可以主导业务规则
* `uc-platform` 不可以承担应用流程编排
* `uc-platform` 不可以向上层泄漏原始 OS API 细节

---

## 4. 核心原则

## 4.1 平台差异必须向下收敛

任何“只有某个系统才这样”的逻辑，都优先考虑收口到 `uc-platform`。

例如：

* Windows 用 `%AppData%`
* macOS 用 `~/Library/Application Support`
* Linux 用 `~/.local/share`

上层不应知道这些差异，只应拿到：

* `app_data_dir()`
* `cache_dir()`
* `runtime_dir()`

---

## 4.2 暴露平台语义，不暴露 OS API 细节

错误做法：

* 上层拿到 `windows::Win32::*` 类型
* 上层知道 macOS bundle path 规则
* 上层自己处理不同系统的 socket 路径生成

正确做法：

* 上层只依赖平台层定义的稳定接口与语义结果
* 平台层内部自行适配各 OS API

---

## 4.3 平台层只处理“系统差异”，不处理“业务真相”

例如：

* “应用数据目录在哪里”属于平台层
* “space 初始化是否完成”不属于平台层
* “系统托盘是否可用”属于平台层
* “何时允许同步剪切板”不属于平台层

---

## 4.4 `cfg` 必须集中，不能扩散

`cfg(target_os = "...")` 是必要的，但它是架构腐蚀点。

必须尽量做到：

* 少量入口文件有 `cfg`
* 平台模块内部实现分文件
* 上层不感知条件编译细节

不允许：

* 在 `uc-app` 各处散落 `cfg`
* 在 core 里出现平台条件分支
* 一个功能的业务流程到处被平台分支切开

---

## 5. `uc-platform` 允许包含的内容

### 5.1 应用目录与路径布局

例如：

* app data dir
* cache dir
* logs dir
* temp dir
* runtime dir
* socket path / named pipe path
* vault root 建议位置

---

### 5.2 平台能力探测

例如：

* 是否支持系统托盘
* 是否支持后台常驻
* 是否支持开机启动
* 是否支持文件拖放
* 是否支持某类 clipboard format
* 是否支持某类通知机制

---

### 5.3 系统集成入口

例如：

* tray
* notification
* launch at login / startup
* single instance lock
* deep link / custom scheme
* file association
* shell integration

---

### 5.4 平台安全存储入口

例如：

* macOS Keychain
* Windows Credential Manager
* Linux Secret Service / Keyring

注意：
这里是“平台入口与平台差异适配”，而不是完整业务 secret 管理逻辑。

---

### 5.5 平台进程与宿主模型

例如：

* daemon 可否常驻
* UI 关闭后是否允许后台继续运行
* 前后台生命周期差异
* 移动端后台限制能力探测

---

### 5.6 平台特有 clipboard / file / shell 行为适配

例如：

* Windows 文件复制语义
* macOS pasteboard 类型差异
* Linux X11 / Wayland 差异入口

---

## 6. `uc-platform` 禁止包含的内容

## 6.1 禁止定义业务规则

不允许在平台层定义：

* 哪些设备可信
* 哪些内容需要同步
* 哪些记录应保留
* 何时允许解锁
* 何时允许加入空间

这些属于 `uc-core` 或 `uc-app`。

---

## 6.2 禁止承担应用流程编排

不允许在平台层处理：

* setup 流程推进
* join 状态机
* pairing 流程
* transfer orchestrator
* search application flow

---

## 6.3 禁止直接变成 infra 的大杂烩

例如不要把这些都堆进平台层：

* sqlite repository
* search index engine
* network protocol implementation
* blob repository
* crypto algorithm implementation

这些属于 `uc-infra`。

---

## 6.4 禁止输出 UI / API 表示层模型

不允许把这些做成平台层公共契约：

* HTTP response DTO
* Tauri command request/response
* 前端 ViewModel
* CLI parser 类型

平台层输出的应该是平台语义结果，而不是表示层数据结构。

---

## 7. `uc-platform` 与 `uc-infra` 的边界

这部分最容易混。

## 7.1 判断标准

可以用一句话判断：

> `uc-platform` 解决“不同系统上有什么差异”，`uc-infra` 解决“某种能力如何具体实现”。

---

## 7.2 例子

### 例 1：应用目录

* “Windows/macOS/Linux 应用数据目录不同” → `uc-platform`
* “在该目录下如何保存 blob 文件” → `uc-infra`

### 例 2：系统密钥存储

* “不同系统 credential store 入口不同” → `uc-platform`
* “如何把某个 secret 保存为业务需要的结构” → `uc-infra`

### 例 3：剪切板

* “Windows/macOS/Linux 访问剪切板 API 不同” → 通常优先放 `uc-platform`
* “读取后如何做标准化、建模、缓存、持久化” → `uc-infra` / `uc-app`

### 例 4：单实例与 IPC 路径

* “Windows named pipe 与 Unix socket 路径规则不同” → `uc-platform`
* “IPC 请求/响应协议如何实现” → `uc-infra`

---

## 8. 模块组织规范

推荐按平台能力组织，而不是按系统名平铺。

推荐结构：

```text
uc-platform/
  src/
    app_dirs/
      mod.rs
      model.rs
      resolver.rs
      windows.rs
      macos.rs
      linux.rs

    runtime/
      mod.rs
      capabilities.rs
      environment.rs
      process_model.rs

    startup/
      mod.rs
      autostart.rs
      windows.rs
      macos.rs
      linux.rs

    secrets/
      mod.rs
      provider.rs
      windows.rs
      macos.rs
      linux.rs

    clipboard/
      mod.rs
      capabilities.rs
      windows.rs
      macos.rs
      linux.rs

    shell/
      mod.rs
      single_instance.rs
      deep_link.rs
      file_association.rs

    notifications/
      mod.rs
      windows.rs
      macos.rs
      linux.rs

    tray/
      mod.rs
      windows.rs
      macos.rs
      linux.rs
```

不推荐这样：

```text
windows/
macos/
linux/
misc/
utils/
helpers/
```

因为这种结构很容易形成：

* 横向能力被切碎
* 跨平台同一能力难对比
* 上层调用点越来越混乱

---

## 9. 平台接口设计规范

## 9.1 对上层暴露稳定抽象

平台层可以有自己的稳定接口，例如：

* `AppDirsProvider`
* `PlatformCapabilitiesProvider`
* `SecretStoreProvider`
* `ClipboardCapabilityProvider`
* `StartupIntegrationProvider`

重点是：
这些接口表达的是平台能力，而不是原始 OS API。

---

## 9.2 不允许把平台原始类型直接暴露给上层

例如不要把：

* Windows HANDLE
* macOS Foundation 对象
* Linux DBus 原始对象

直接暴露给 `uc-app` 或 `uc-core`。

必须在平台层内部消化。

---

## 9.3 平台能力应显式表达“不支持”

不同平台能力不一致时：

* 不要假装所有平台都支持
* 不要偷偷降级不告诉上层
* 不要靠 panic 处理“不支持”

应该明确返回：

* `Unsupported`
* `Unavailable`
* `NotConfigured`
* `PermissionDenied`

之类的语义化结果。

---

## 10. 条件编译规范

## 10.1 `cfg` 只在必要位置出现

推荐：

* `mod windows;`
* `mod macos;`
* `mod linux;`

再在统一入口选择实现。

不推荐：

* 大量函数内部到处 `#[cfg(...)]`
* 一个文件里穿插大量平台分支
* 上层调用者自己写平台分支

---

## 10.2 平台差异必须在统一边界后收敛

例如：

```rust
pub trait AppDirsProvider {
    fn app_data_dir(&self) -> Result<PathBuf, AppDirsError>;
    fn cache_dir(&self) -> Result<PathBuf, AppDirsError>;
}
```

Windows / macOS / Linux 各自实现，但上层只看到统一接口。

---

## 10.3 禁止平台分支上浮到 `uc-core`

`uc-core` 中严禁出现：

* 操作系统判断
* 平台路径规则
* 平台 API 假设
* 桌面/移动平台条件逻辑

---

## 11. 错误处理规范

## 11.1 平台错误必须语义化

不要把原始 OS 错误直接抛给上层作为公共错误。

错误示例：

* Win32 error code 原样上抛
* Foundation NSError 原样上抛
* DBus 原始错误字符串一路外泄

正确做法：

```rust
pub enum AppDirsError {
    Unsupported,
    Unavailable,
    PermissionDenied,
    InvalidPlatformState,
    Io(String),
}
```

---

## 11.2 不允许静默降级

平台特性失败时，不能“悄悄算了”。

例如：

* 开机启动注册失败
* 单实例锁失败
* 通知发送失败
* keychain 不可用
* runtime dir 无法创建

必须：

* 有日志
* 有清晰错误
* 由上层决定是否继续、提示或降级

---

## 11.3 不允许把平台怪异行为直接传播给上层

平台层的职责之一，就是把“系统很怪”的部分消化掉。

上层不应被迫理解：

* Windows 某个错误码是什么意思
* Wayland 为什么拿不到某种格式
* macOS 某个权限没给时 Foundation 返回什么

这些都应转换成稳定语义。

---

## 12. 日志与 tracing 规范

## 12.1 平台层必须可观测

关键平台行为必须有日志：

* 目录解析
* 权限检查
* 能力探测
* 单实例锁创建
* 开机启动注册
* keychain / credential store 接入
* 托盘 / 通知初始化
* 平台 API 调用失败

---

## 12.2 日志应面向平台排障

日志要回答：

* 当前在哪个平台
* 调用的哪项平台能力
* 使用了哪条系统路径 / 机制
* 失败发生在哪个阶段
* 是否属于不支持、权限问题还是环境异常

---

## 12.3 不得泄露敏感信息

严禁打印：

* 明文 secret
* 明文 passphrase
* 可恢复用户数据的 payload
* 用户私密路径中的敏感片段（需要视情况脱敏）

---

## 13. 测试规范

## 13.1 平台层必须重视“行为契约测试”

重点不是“系统 API 调通了”，而是：

* 对上层提供的语义是否稳定
* 不同平台下同一能力是否行为一致
* 不支持时是否返回正确语义
* 权限不足时是否有清晰错误

---

## 13.2 优先测试平台适配边界

例如：

* `app_data_dir()` 是否总能返回统一语义
* `secret_store_available()` 是否正确反映支持情况
* `single_instance_lock()` 失败时是否返回明确错误

---

## 13.3 必须覆盖“不支持 / 权限不足 / 环境异常”

平台层不能只测 happy path。

必须覆盖：

* 平台不支持
* 权限被拒绝
* 系统目录不可写
* 运行环境异常
* 依赖服务缺失
* 桌面环境不存在
* Wayland/X11 差异
* 后台能力被宿主限制

---

## 14. 性能与资源规范

## 14.1 平台层要避免重复探测与无意义系统调用

例如：

* 每次都重新解析系统目录
* 每次都重新探测托盘支持
* 每次都重新计算运行环境能力

应视情况：

* 做受控缓存
* 明确缓存生命周期
* 明确能力探测时机

---

## 14.2 平台资源必须可释放

例如：

* 单实例锁
* 托盘句柄
* 系统 watcher
* 剪切板监听句柄
* 桌面通知对象

不能只创建不释放，不能依赖进程退出兜底。

---

## 14.3 平台 watcher / listener 必须可停止

任何平台监听器都必须：

* 可启动
* 可停止
* 失败可见
* 生命周期明确

不能“启动了就放那儿”。

---

## 15. 依赖管理规范

## 15.1 平台依赖要谨慎

引入新的平台库前，必须回答：

1. 这是平台差异问题，还是 infra 能力问题？
2. 是否必须引入该库？
3. 是否会加重某个平台构建负担？
4. 是否会让条件编译复杂度失控？
5. 是否会把平台类型泄漏到上层？

---

## 15.2 优先选择边界清晰的依赖

优先：

* 平台覆盖明确
* 错误行为清楚
* 生命周期清晰
* 文档完整
* 可局部封装

谨慎：

* 宏过重
* 全局状态很强
* 平台副作用不透明
* 需要大量 unsafe 且边界难以收敛

---

## 16. 命名规范

| 类型    | 规则                    | 示例                             |
| ----- | --------------------- | ------------------------------ |
| 提供者接口 | `*Provider`           | `AppDirsProvider`              |
| 能力接口  | `*CapabilityProvider` | `PlatformCapabilitiesProvider` |
| 平台适配器 | `*Adapter`            | `WindowsSecretStoreAdapter`    |
| 错误    | `*Error`              | `StartupIntegrationError`      |
| 能力结果  | `*Capabilities`       | `ClipboardCapabilities`        |

避免模糊命名：

* `PlatformService`
* `SystemManager`
* `Helper`
* `Utils`
* `EnvService`

---

## 17. 提交前自我审查清单

每次修改 `uc-platform`，必须逐项自查：

### 17.1 边界检查

* [ ] 这次修改处理的是平台差异，而不是业务规则或应用流程？
* [ ] 是否把本应属于 infra 的实现塞进了 platform？
* [ ] 是否把平台细节向上层泄漏了？

### 17.2 `cfg` 检查

* [ ] 条件编译是否收敛在平台层内部？
* [ ] 是否避免了平台分支上浮到 app/core？
* [ ] 是否存在散乱、重复、难维护的 `cfg` 分支？

### 17.3 接口检查

* [ ] 对上层暴露的是平台语义，而不是原始 OS API？
* [ ] 是否显式表达了“不支持 / 不可用 / 权限不足”？
* [ ] 是否存在上层必须理解平台怪异细节的问题？

### 17.4 错误与日志检查

* [ ] 是否对平台错误做了语义化转换？
* [ ] 是否存在静默降级或吞错？
* [ ] 是否增加了足够的排障日志？

### 17.5 测试检查

* [ ] 是否覆盖不支持、权限不足、环境异常？
* [ ] 是否验证了统一平台语义，而不是只验证某个 API 调用成功？
* [ ] 是否避免测试依赖特定本机环境偶然成功？

---

## 18. Code Review 重点

评审 `uc-platform` 时，优先检查：

1. 是否真的在处理平台差异，而不是混入业务或 infra 逻辑
2. 是否把 `cfg` 收敛住了
3. 是否有平台细节泄漏给上层
4. 是否对“不支持”场景处理明确
5. 是否有静默降级
6. 是否有资源句柄泄漏或 watcher 生命周期失控
7. 是否把平台怪异行为很好地翻译成稳定语义

---

## 19. 反模式清单

### 19.1 把 `uc-platform` 当成“系统工具箱”

什么跟系统沾边都塞进来，最后没有边界。

---

### 19.2 平台分支散在全仓库

到处都是 `cfg(target_os = ...)`，最后谁都不敢改。

---

### 19.3 平台原始类型一路往上漏

上层开始知道 HANDLE、DBus、Foundation、named pipe 细节，这说明平台层失守了。

---

### 19.4 平台层偷偷做业务决定

例如因为某个平台能力有限，就在平台层直接决定“那这个业务流程不走了”。
这类决定应该交给 app 层。

---

### 19.5 用“静默降级”掩盖平台失败

这种最危险。表面兼容，实际状态不可观测。

---

## 20. 总原则

`uc-platform` 必须遵守这四条：

### 20.1 收口平台差异，而不是扩散平台差异

### 20.2 暴露平台语义，而不是原始 OS 细节

### 20.3 不定义业务真相，也不承担应用流程

### 20.4 让上层像面对统一平台一样工作

---

## 21. 一句话原则

> `uc-platform` 的职责不是“直接操作系统 API”，而是“把不同宿主和操作系统的差异收敛成上层可稳定依赖的平台语义”。`
