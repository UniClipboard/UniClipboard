# 核心诊断与桌面日志装配

本接入需与包含 `install_with_host_layers` 的 Engine 修订一起使用。当前工作分支固定使用包含该接口的 Engine 提交；发布前仍需等待 Engine PR 合并并按仓库发布流程确认版本。

## 所有权

`crates/uc-bootstrap/src/observability/tracing.rs` 通过稳定的 `uc-engine::observability` 入口一次安装进程日志。

- Console、桌面 JSON 和 Sentry 仍由桌面宿主构造，作为宿主层交给共同运行时。
- Engine 的完成记录、详细失败、在线检查和生命周期记录由共同运行时编码。宿主层不重复接收核心事件或其网络依赖的原始诊断。
- 只有 daemon 写入平台日志目录中的 `engine.YYYY-MM-DD.jsonl`。GUI 与 CLI 是客户端角色，不与 daemon 混写这些文件。
- 新的 Engine 文件与现有角色文件由已有诊断导出同时收集；Engine 导出入口先刷新共同文件队列。

## 隐私与远程输出

核心详细原因只进入本地诊断文件；认证完成仍只有一条记录，认证前不写跨设备关联。宿主 Sentry 层继续处理桌面宿主自身的错误、日志和性能记录，不接收核心原始记录。本接入不把 Sentry 开关解释成新增 Engine 远程诊断许可，也不隐式配置 Collector。

本地核心文件的字段合同、保留策略和编码归 Engine 仓所有。桌面不复制具体失败分类、序列化或关联逻辑。

## 验证范围

- 共同运行时的宿主组合测试验证应用日志保留、核心日志不重复、原始网络输出不能绕过过滤。
- 桌面测试验证 daemon/GUI/CLI 文件所有权、固定资源分类和默认远程关闭。
- 配对请求与实际文件的详细失败测试位于 Engine 仓。
- 本接入不代表前端到 daemon 的完整跨进程观测规格已经完成；Windows/iPhone 实机与发布验证仍须分别执行。
