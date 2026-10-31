# 按历史条目读取缩略图

## 当前契约

Desktop 通过 Engine 的 `Operation::ReadEntryThumbnail(HistoryEntryInput)` 读取缩略图。
HTTP 路径为 `GET /clipboard/entries/{id}/thumbnail`。`id` 是历史条目编号。
旧路径 `/clipboard/thumbnails/{rep_id}` 已移除。
本机控制面版本 `DAEMON_API_REVISION` 升至 `2`，避免旧 daemon 继续服务新客户端。

Engine 负责解析历史条目的图片表示、限制解码资源、生成缩略图并检查加密会话。
Desktop 只转换 HTTP 请求和响应，不自行查找表示编号，也不恢复后台缩略图生成。

| HTTP 状态 | 含义 |
| --- | --- |
| `200` | 返回缩略图字节和对应的媒体类型 |
| `404` | 纯文本错误：条目不存在，或条目没有可读取的图片 |
| `503` | Engine 当前不可用；JSON 中的 `details.retryable` 指示是否可以重试 |
| `500` | 纯文本错误：内部读取失败 |

加密会话锁定时，Engine 拒绝读取缩略图。Desktop 保留 Engine 错误编号、分类和可重试标志。
调用方不得把锁定或忙碌结果永久缓存为「没有缩略图」。解锁后可以重新请求。
GUI 类客户端仍须通过 Desktop 的内容锁检查。

历史列表不再包含 `thumbnailUrl`、`imageWidth` 或 `imageHeight`。
现有界面按条目读取图片资源，并从解码结果取得图片比例。
完整图片资源接口保持独立；缩略图不作为完整图片内容。

## 验证

先运行 `cargo build -p uc-daemon`。再执行：

```bash
python3 scripts/e2e/entry-thumbnail.py --out target/entry-thumbnail-evidence
```

脚本启动真实 daemon，使用独立临时资料目录，并禁用系统剪贴板。
脚本验证新路径、旧路径移除、加密会话锁定、解锁恢复和正常退出。
`receipt.json` 保存结果和二进制摘要。脚本不执行界面验收。
