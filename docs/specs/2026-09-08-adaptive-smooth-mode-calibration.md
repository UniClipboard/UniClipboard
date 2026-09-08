# 流畅模式能力初判与校准记录

日期：2026-09-08。此次修复替换了「读到 CPU/内存但总是返回 unknown」的占位实现。

## 已采用的初判规则

必须同时满足：物理核心数至少 4、物理内存至少 8 GiB、原生现代硬件图形能力可用。资源门槛用于保守初判，不代表所有满足条件的设备都拥有相同帧率；现有真实交互评估仍能在重复卡顿后降低下一次启动效果。

| 平台 | 图形信号 | 降级规则 |
| --- | --- | --- |
| macOS | 枚举 Metal 设备，所有可用设备均支持 Mac2 GPU family | 旧能力为 constrained；没有设备为 unknown；不只挑选最强 GPU |
| Windows | 默认 HARDWARE 适配器能创建至少 feature level 12_0 的 Direct3D 设备 | 较旧硬件级别为 constrained；创建失败为 unknown；禁止软件驱动回退 |
| Linux | 不执行探测 | 自动固定流畅，手动效果优先仍可用 |

已知资源不足时直接选择 constrained；缺失或读取失败时选择 unknown，二者都会保守流畅。只有三个条件都满足时选择 capable。

原生能力表示硬件支持的功能，不等同于当前 WebView 的实际合成器，也不等同于实测帧率。开发版和正式版使用相同规则；没有 Mac mini 或 Apple M4 型号白名单。

## 本机证据

- 当前测试机：Apple M4，10 个物理核心，24 GiB 内存。
- 原生检测已经返回 `Capable`；第一次独立宿主测试约 723 ms，后续 shell 测试约 73 ms。
- 500 ms 可能截断冷启动检测，因此等待上限改为 2 秒。检测在后台执行，等待时界面采用静态效果；超时只影响本次自动选择。
- 初判、资源边界、旧图形能力、缺失数据、旧策略迁移、手动覆盖与系统减少动效均由定向测试验证。
- Windows 与 Linux 的能力模块已通过相应目标的编译检查；这不是 Windows/Linux 真机运行证明。
- 原生 GUI 已通过独立 `smooth-auto-m4-check` 配置验证：自动返回 `result=effects`、`reason=hardware`、`lowEffects=false`；手动切换与窗口重新加载保持同一会话的判断结果。运行器输出了已有的外部驱动/清理警告，但嵌入式 WebKit 断言通过且退出码为零。

## 保存与升级

策略版本为 2。仍只保存粗分类与模式，不保存 CPU/GPU 型号或原始硬件数据。策略版本变化清除旧自动覆盖，但不覆盖用户的手动模式，不修改配对和历史。

## 尚需的广泛校准

代表性 Windows/Intel Mac、强弱设备对照、不同 WebView/驱动配置和监测开销实测仍需按 spec S5 补充。本次已让具备可靠能力证据的设备实际启用完整效果，不再把缺少全部设备校准当作所有设备永久 unknown 的理由。

## API 依据

- [Apple Metal 能力表](https://developer.apple.com/metal/capabilities/)：GPU family 表达能力范围。
- [Apple MTLCopyAllDevices](https://developer.apple.com/documentation/metal/mtlcopyalldevices())：取得 Metal 设备集合。
- [Microsoft D3D11CreateDevice](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-d3d11createdevice)：HARDWARE 驱动、能力级别数组及旧运行时的参数错误处理。

CPU/内存门槛是本产品的保守资源规则，不是上述文档给出的性能阈值。
