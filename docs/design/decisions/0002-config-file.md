# 0002 配置文件:TOML + 保存即生效

## 决定

- 配置文件是 `$UNIC_HOME/config.toml`(默认 `os.UserConfigDir()/unic/config.toml`)。首次启动时写入一份全部注释掉的模板。
- 格式 TOML。未知键视为错误(防止拼写错误悄悄失效)。
- daemon 监听配置文件所在**目录**(编辑器保存常是"写临时文件再改名"),防抖 150ms 后重新加载。
- 坏配置(语法错、未知键、取值非法、文件消失):**保留上一份有效配置**,记录错误,发 `config` 事件。daemon 不退出,也不回落到默认值。
  唯一例外是启动时:此时没有"上一份",配置有错则拒绝启动并说明原因。
- `unic config` 显示当前生效的配置、文件路径和最近一次加载错误;`unic config path` 只打印路径。API:`GET /v1/config`。

## 可配置项

| 键 | 默认 | 生效时机 |
|---|---|---|
| `device_name` | 主机名 | 之后的新连接(握手);已连接的会话重连后才更新 |
| `rendezvous_url` | `UNIC_RENDEZVOUS_URL`,再不然内置地址 | 下一次发码/用码 |
| `inbox` | `$UNIC_HOME/inbox` | **需重启**(状态里列为 restart_required) |
| `limits.max_file_bytes` | 2GB | 之后的新传输;发送与接收都按它检查 |
| `limits.max_total_bytes` | 4GB | 同上 |
| `sync.enabled` | true | 立即(false = 不发不收,连接保留) |
| `sync.text` / `sync.images` / `sync.files` | true | 立即 |

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
- `sync.text` 管 `image/*` 以外的所有内存格式(文本、HTML 等);`sync.images` 管 `image/*`;`sync.files` 管文件。
  过滤后为空的剪贴板不发送/不应用。
- 自定义 `inbox` 是用户的目录:启动清理只删除其中 `clip-*` 子目录,绝不整目录删除。
