# 0002 配置文件:TOML + 保存即生效

## 决定

- 配置文件是 `$UNIC_HOME/config.toml`(默认 `os.UserConfigDir()/unic/config.toml`)。首次启动时写入一份全部注释掉的模板。
- 格式 TOML。未知键视为错误(防止拼写错误悄悄失效)。
- daemon 监听配置文件所在**目录**(编辑器保存常是"写临时文件再改名"),防抖 150ms 后重新加载。
- 坏配置(语法错、未知键、取值非法、文件消失):**保留上一份有效配置**,记录错误,发 `config` 事件。daemon 不退出,也不回落到默认值。
  唯一例外是启动时:此时没有"上一份",配置有错则拒绝启动并说明原因。
- `unic config` 显示当前生效的配置、文件路径和最近一次加载错误;`unic config path` 只打印路径。API:`GET /v1/config`。

## 0007 目标扩展（待实现）

[0007](0007-multiple-sync-backends.md) 将目标配置改为全局 `sync.enabled` + 每同步插件的 `.sync` 策略，支持多个后端独立的 text/images/files 与 auto_file_max_bytes。**已实现（2026-11-08）**：`sync.text/images/files` 已删除（出现即“未知键”错误，不做兼容映射），类型开关与自动文件阈值改为 `[plugins.instances."builtin.tailcat".sync]`；桌面目前只接受 `builtin.tailcat` 这一个实例。全局关闭按 0007 立即取消所有后端在途内容同步；下文“不追溯在途”只继续适用于普通限额和后端策略变更。

## 0014 日志与遥测扩展（已接入工作区）

[0014](0014-client-observability-otlp.md) 新增 `[logging]` 与 `[telemetry]` 配置：本机和远程日志等级分别热更新，远程遥测默认关闭，本次总开关与 logs/metrics 开关热更新，端点修改需重启；traces 与采样率留待后续实施。非法配置沿用上一有效值。logging、telemetry、logs 与 metrics 键已接入解析器。Traces 键仍按未知键拒绝。

本机 CLI/socket API 另提供限时日志等级覆盖，不改写 TOML；覆盖到期/reset 恢复最新有效配置，重启清除。`unic config` / `GET /v1/config` 展示持久配置，当前有效等级和覆盖状态经诊断 API 查询；完整默认值和控制语义见[可观测性契约](../../contracts/observability-contract.md)，验收见[0014 执行计划](../../exec/0014-client-observability-otlp.md)。

## 可配置项

| 键 | 默认 | 生效时机 |
|---|---|---|
| `device_name` | 主机名 | 之后的新连接(握手);已连接的会话重连后才更新 |
| `rendezvous_url` | `UNIC_RENDEZVOUS_URL`,再不然内置地址 | 下一次发码/用码 |
| `inbox` | `$UNIC_HOME/inbox` | **需重启**(状态里列为 restart_required) |
| `limits.max_file_bytes` | 2GB | 之后的新传输;发送与接收都按它检查 |
| `limits.max_total_bytes` | 4GB | 同上 |
| `sync.enabled` | true | 立即(false = 不发不收,连接保留) |
| `plugins.instances."builtin.tailcat".sync.text` / `.images` / `.files` | true | 立即(之后开始的传输;接收同样按它过滤) |
| `plugins.instances."builtin.tailcat".sync.auto_file_max_bytes` | 0 | 立即;只限制自动发送,任一文件超过则该次自动提交整体不发,手动发送不受限;0 = 无额外限制 |
| `history.max_items` | 0 | Live; 0 means unlimited eligible records; maximum 1000000 |
| `history.max_bytes` | 4GiB | Restart required; counts history-owned bytes; host references do not count; maximum 8GiB |
| `history.retention_days` | 180 | Live; 0 disables age cleanup |
| `history.auto_cleanup` | true | Live; unified history and managed-file cleanup |
| `history.excluded_tags` | [] | Live; any exact matching tag excludes the record and its managed files |

大小可写整数(字节)或带单位的字符串(`"512MB"`、`"2GiB"`);上限不得超过协议硬上限(2GB / 4GB)。

## 不开放的

- `UNIC_HOME`、`UNIC_SOCK`:必须在读配置之前确定,继续用环境变量。
- 传输端口:改了会让地址和已配对设备失效。
- 协议版本、超时、退避、`MaxParts`、单格式 64MB 上限:内部细节。
- 设备地址、配对票据等秘密:不进配置文件,也不进配置错误信息。

## 行为细节

- 对已在进行的传输不追溯:缩小限制只影响之后开始的传输。
- 接收方超限时,丢弃该剪贴板的数据并保持连接(读完并丢弃已声明的字节),而不是断开会话;
  否则两端限制不一致会造成断线重连循环。
- `text` 管 `image/*` 以外的所有内存格式(文本、HTML 等);`images` 管 `image/*`;`files` 管文件。
  过滤后为空的剪贴板不发送/不应用。
- 自定义 `inbox` 是用户的目录:启动清理只删除其中 `clip-*` 子目录,绝不整目录删除。

## Desktop startup settings

`[desktop]` stores launcher-only intent in the same strict TOML document.
`mode = "normal"`, `launch_at_login = false`, and `start_in_background = false` are the defaults.
Normal mode owns a fresh service generation. Lightweight mode leaves only a standalone service.
The service validates and stores these fields without executing desktop operations.
See [startup architecture](../../architecture/headless.md#desktop-startup-settings) for lifetime, change application and platform boundaries.
