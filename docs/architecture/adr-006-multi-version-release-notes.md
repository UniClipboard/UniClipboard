# ADR-006：多版本发布日志归档与拼接

- **状态**：Draft（提案中）
- **日期**：2026-05-23
- **相关文档**：`docs/release-workflow.md`、`workers/update-server/src/index.ts`、`.github/workflows/release.yml`、`scripts/assemble-update-manifest.js`

## 1. 背景

### 1.1 现状

发布管线已经把以下能力跑通：

| 组件 | 路径 | 职责 |
|---|---|---|
| GitHub Actions | `.github/workflows/release.yml` | 构建产物 → 上传 R2 / GitHub Release → 部署 gh-pages |
| 归档脚本 | `scripts/assemble-update-manifest.js` | 把产物的 `.sig` 扫描成 `platforms` + 把 `<en>\n\n<!-- zh -->\n\n<zh>` 注入到 `notes` 字段 |
| R2 | `manifests/<channel>.json`、`artifacts/v<ver>/<file>` | 唯一可信的发布载荷源 |
| Cloudflare Worker | `workers/update-server/src/index.ts` | `GET /<channel>.json` → R2 透传 manifest；`GET /artifacts/v<ver>/<file>` → R2 透传二进制 |
| GitHub Pages | `<channel>.json`（fallback 镜像） | Tauri updater 的备用 endpoint |
| Tauri 客户端 | `src-tauri/tauri.conf.json` updater.endpoints | `https://release.uniclipboard.app/<channel>.json` → 失败回退到 gh-pages |

每次发布只重写 **一份** `<channel>.json`，里面的 `notes` 只描述当前版本。

### 1.2 触发需求

用户从 `v0.10.0-alpha.3` 跨版本升级到 `v0.11.0-alpha.6`，Tauri updater 对话框只能看到 `v0.11.0-alpha.6` 的 notes。中间几个版本的变更（含潜在 breaking change、迁移说明）对用户完全不可见。

希望：

1. 每个版本的发布日志被独立归档保存，可单独查询
2. 客户端从「当前版本」升级到「最新版本」时，自动获得 **所有中间版本** 的合并 notes
3. 拼接有上限（最多 5 个版本），避免大跨度升级时 manifest 过大

### 1.3 关键约束（已验证）

- **Tauri updater 2.10.1 支持 endpoint URL 模板** — `updater.rs:413-439` 中确认 `{{current_version}}`、`{{target}}`、`{{arch}}` 都会被原地替换（含 URL 编码版本 `%7B%7B...%7D%7D`）。这是「零客户端改动」方案的基石。
- **R2 无事务** — 索引文件并发更新有理论上的丢失风险；实际上 GH Actions 串行化发布，可忽略，必要时从 R2 list 重建。
- **manifest 大小** — Tauri updater 期望同步解析返回的 JSON，合并 5 个版本的 notes（每个约 1–4 KB）后总大小 ≈ 10–20 KB，可接受。
- **客户端版本可能不在索引中** — 老版本、本地 dev build、跳跃式安装都可能让 `from` 在索引里找不到；不能因此中断升级流程。

## 2. 决策

### 2.1 核心思路

**保留并增强现有 `/<channel>.json` 路由**，让它在收到 `?from=<version>` 时返回「合并 notes 后的 manifest」，**不引入第二条客户端路由**。

这样 Tauri updater 对话框无需任何代码改动即可显示合并 notes —— 只改一行 `tauri.conf.json` 配置。

### 2.2 R2 目录结构

```
uniclipboard-releases/
├── manifests/
│   ├── stable.json                    # 现有 — 当前最新版 manifest（含单版 notes，用作 from 缺省兜底）
│   ├── alpha.json
│   ├── beta.json
│   └── rc.json
├── artifacts/                         # 现有 — 二进制
│   └── v<version>/<file>
└── release-notes/                     # 新增
    ├── v<version>.json                # 单版本完整归档（结构见 §2.3）
    └── index/
        ├── stable.json                # 每个 channel 一份有序版本索引（结构见 §2.4）
        ├── alpha.json
        ├── beta.json
        └── rc.json
```

### 2.3 单版本归档文件 `release-notes/v<version>.json`

```json
{
  "version": "0.11.0-alpha.6",
  "channel": "alpha",
  "pub_date": "2026-05-22T10:30:00Z",
  "notes_en": "## What's Changed\n- feat(file-transfer): ...",
  "notes_zh": "## 主要变更\n- feat(file-transfer)：..."
}
```

**为什么 `notes_en` / `notes_zh` 分开而不是已合并的 `<!-- zh -->` 串**：拼接时 Worker 可以自由决定按语言分组、还是按版本分组，未来 i18n 拓展也不绑死格式。

### 2.4 Channel 索引文件 `release-notes/index/<channel>.json`

```json
{
  "channel": "alpha",
  "updated_at": "2026-05-22T10:30:00Z",
  "versions": [
    { "version": "0.11.0-alpha.6", "pub_date": "2026-05-22T10:30:00Z" },
    { "version": "0.11.0-alpha.5", "pub_date": "2026-05-20T08:15:00Z" },
    { "version": "0.11.0-alpha.4", "pub_date": "2026-05-15T14:00:00Z" }
  ]
}
```

- 按 `pub_date` **降序**（最新在前）
- **只列本 channel 的版本** — stable 用户跨版本升级不会看到中间的 alpha
- 写入时机：`release.yml` 在上传完单版本归档之后追加。读取 → 排序 → 重写

### 2.5 Worker 路由设计

| 方法 | 路由 | 行为 |
|---|---|---|
| GET | `/<channel>.json` | **不带 `?from=`**：透传 `manifests/<channel>.json`（与现有行为完全一致，零回归） |
| GET | `/<channel>.json?from=<version>` | 拼接版本 `(from, latest]` 的 notes（最多 5 个，硬编码不可调）；返回结构同 manifest，仅 `notes` 字段被替换 |
| GET | `/release-notes/v<version>.json` | 透传 `release-notes/v<version>.json`（给文档站、独立的 changelog 浏览页用） |
| GET | `/release-notes/<channel>.json` | 透传 `release-notes/index/<channel>.json`（让前端能列出所有历史版本） |
| GET | `/artifacts/v<ver>/<file>` | 现有 — 不变 |
| GET | `/health` | 现有 — 不变 |

#### Cloudflare cache key 必须包含 query string

Cloudflare 边缘缓存的 **默认 cache key 不包含 query string**。如果不处理，`?from=v0.10.0` 与 `?from=v0.11.0` 会撞到同一个边缘缓存项，第一次命中的版本会污染后续所有不同 `from` 的请求 —— 这是必须在 Worker 里显式处理的生产事故源。

选定方案：**用 Workers Cache API 显式以 `request.url`（含 query string）作为 cache key**：

```ts
const cache = caches.default
const cacheKey = new Request(request.url, request)   // 完整 URL 作 key
let response = await cache.match(cacheKey)
if (!response) {
  response = await buildMergedManifestResponse(channel, fromVersion, env)
  ctx.waitUntil(cache.put(cacheKey, response.clone()))
}
return response
```

不依赖 `cf: { cacheKey }` 选项（Workers 调用 R2 时不走该路径，且不同 plan 行为不一致）。`max-age=60` 保留，每 60 秒重算一次拼接。

### 2.6 拼接算法（Worker 端）

伪代码：

```ts
const MAX_MERGE = 5   // 硬编码上限，不暴露给调用方

async function mergeNotes(channel, fromVersion, env) {
  const latestManifest = await getR2Json(`manifests/${channel}.json`)
  const index = await getR2Json(`release-notes/index/${channel}.json`)

  const fromIdx = index.versions.findIndex(v => semverEq(v.version, fromVersion))

  // Edge case A: from 不在索引（老版本、dev build、未来未知版本）→ 回落到「只返回最新版 notes」
  if (fromIdx === -1) {
    return { manifest: latestManifest, truncated: false, mergedCount: 1 }
  }

  // Edge case B: from 已经是最新版（fromIdx === 0）→ candidates 为空，行为与无 ?from= 完全一致
  // 直接返回 latestManifest，避免空拼接产生空 notes
  if (fromIdx === 0) {
    return { manifest: latestManifest, truncated: false, mergedCount: 1 }
  }

  // index 按降序排列，所以 (from, latest] = index[0..fromIdx)
  const candidates = index.versions.slice(0, fromIdx)   // 不含 from 本身
  const selected = candidates.slice(0, MAX_MERGE)
  const truncated = candidates.length > MAX_MERGE
  const omittedCount = Math.max(0, candidates.length - MAX_MERGE)

  // 并发拉取每个版本的归档
  const archives = await Promise.all(
    selected.map(v => getR2Json(`release-notes/v${v.version}.json`))
  )

  // 拼接：按版本降序，每个版本一段，标题用版本号
  const mergedNotes = buildCombinedNotes(archives, { truncated, omittedCount, fromVersion })

  return {
    manifest: { ...latestManifest, notes: mergedNotes },
    truncated,
    mergedCount: selected.length,
  }
}
```

`buildCombinedNotes` 输出结构（en + zh 都拼接，沿用现有 `<!-- zh -->` 分隔符以兼容下游 markdown 渲染）：

```markdown
> 本次升级跨越了 3 个版本，以下是按版本倒序的累计变更（从 v0.11.0-alpha.3 升级）。

## v0.11.0-alpha.6

<notes_en_for_a6>

## v0.11.0-alpha.5

<notes_en_for_a5>

## v0.11.0-alpha.4

<notes_en_for_a4>

<!-- zh -->

> 本次升级跨越了 3 个版本……

## v0.11.0-alpha.6

<notes_zh_for_a6>

…
```

### 2.7 Semver 排序

**不要用字符串排序** —— `"0.10.0" < "0.9.0"` 字典序错误，`"0.10.0-alpha.5" vs "0.10.0"` 也需要预发布规则。Worker 用 [`semver`](https://www.npmjs.com/package/semver) npm 包做 `compare()`。

实际排序入口在写入阶段（`release.yml` 调脚本更新索引时），Worker 读取时 **信任 index 已排好序**，只做线性扫描即可。

### 2.8 客户端配置改动

`src-tauri/tauri.conf.json`：

```json
"endpoints": [
  "https://release.uniclipboard.app/stable.json?from={{current_version}}",
  "https://uniclipboard.github.io/UniClipboard/stable.json"
]
```

- 主 endpoint 加上 `?from={{current_version}}` —— Tauri updater 会在每次检查更新时把 `{{current_version}}` 替换成当前安装版本
- gh-pages 备用 endpoint **不** 加 `?from=`（gh-pages 是静态文件，不支持查询参数动态合成）—— 退化为单版本 notes，作为 Worker 整体不可用时的降级路径

### 2.9 发布流程改动

`release.yml` 在「Upload artifacts to R2」与「Assemble and upload R2 manifest」之后追加一个新步骤「Archive release notes & update channel index」，做三件事：

1. 把 `docs/changelog/<version>.md` 和 `docs/changelog/<version>.zh.md` 打包成 `release-notes/v<version>.json` 上传 R2
2. 读取 `release-notes/index/<channel>.json`（不存在则视为空）
3. 在 `versions` 列表头部插入新版本 → semver 排序去重 → 写回 R2

由新脚本 `scripts/archive-release-notes.js` 完成。

## 3. 影响

### 3.1 改动清单

| 文件 / 位置 | 改动 |
|---|---|
| `workers/update-server/src/index.ts` | 增加 `?from=` 参数处理、新增 3 个路由（`/<channel>.json` 增强、`/release-notes/v*.json`、`/release-notes/<channel>.json`） |
| `workers/update-server/package.json` | 新增依赖 `semver` |
| `scripts/archive-release-notes.js` | 新文件 — 上传单版本归档 + 更新 channel 索引 |
| `scripts/__tests__/archive-release-notes.test.ts` | 新文件 — 单元测试，覆盖 semver 排序、并发安全标注、from 不在索引的边界 |
| `.github/workflows/release.yml` | 在 R2 步骤后追加「Archive release notes & update channel index」 |
| `src-tauri/tauri.conf.json` | endpoint 加 `?from={{current_version}}` |
| `docs/release-workflow.md` | 文档增补「跨版本 notes 拼接行为」一节 |

### 3.2 兼容性

- **未升级的客户端**（老版 Tauri 配置无 `?from=`）：Worker 检测到无参数 → 走原路径透传 `manifests/<channel>.json` → 行为完全一致 ✅
- **gh-pages 备用 endpoint**：始终是单版本 notes，作为降级路径可接受 ✅
- **R2 manifest 文件**（被其他工具脚本读取）：完全不变 ✅
- **历史已发布版本**（无单版本归档）：见 §3.4 回填策略

### 3.3 监控与可观测

Worker 在拼接路径上添加 `console.log` 关键日志（落到 Cloudflare Logs）：
- `from` 版本是否命中索引
- 实际拼接版本数与 `truncated` 标记
- 拼接耗时（并发 R2 read 总耗时）

如果未来出现频繁 truncation，说明上限 5 不够，可调整。

### 3.4 历史数据回填

发布该方案后，**不** 回填历史版本的归档文件。原因：

- 老客户端升级时即使 `from` 未命中，Worker 也会回退到「只返回最新版 notes」（不破坏升级流程，§2.6）
- 一旦从「方案上线后第一次发布」开始，索引就在增长，跨版本升级 notes 自然累积
- 回填的边际收益不值得：跨大版本升级看到的依然是最近 N 个新版本的变更

如果未来确实需要补历史，单独写一次性 backfill 脚本，遍历 `docs/changelog/` 上传到 R2 即可。

## 4. 备选方案与拒绝原因

### 4.1 独立的 `/release-notes/<channel>?from=...` 路由

**拒绝原因**：Tauri 内置 updater 对话框只读它请求的 manifest 里的 `notes` 字段，独立路由意味着必须在 `uc-tauri` 写额外的 UI 代码去拉取并展示合并 notes，工作量和回归面都大很多。

### 4.2 客户端拉到本地后再做拼接

**拒绝原因**：客户端要发 N+1 个请求（先拉 index 再拉每个版本），网络和复杂度都更高；而且要在 Rust 端实现 markdown 拼接逻辑，跨平台和 i18n 都是负担。

### 4.3 把所有版本的 notes 直接塞进单一 `manifests/<channel>.json`

**拒绝原因**：体积无限增长；新版本 manifest 写入需要先读旧版再追加，并发竞态比独立索引更严重；客户端总是收到完整历史，浪费带宽。

### 4.4 用 R2 `list` API 替代 index 文件

**拒绝原因**：list 调用慢、贵、需要客户端排序；index 文件只是一行追加，读起来 O(1)，符合现有「Worker 直读 R2」模式。

## 5. 实施顺序（建议）

1. **Worker 端先落地** — `?from=` 参数处理、新路由、单元测试，不影响线上行为（无客户端调用 `?from=`）
2. **新增归档脚本 + release.yml** — 接下来每次发布开始累积归档
3. **客户端配置改 endpoint** — 拼到下一个 alpha release 一起发出去，老客户端继续走兼容路径
4. **观察一段时间**（≥ 3 个 alpha 周期），确认拼接逻辑稳定后写进 `docs/release-workflow.md`

## 6. 待确认事项

- [ ] 拼接 notes 的 markdown 模板细节（每段标题用 `## v<version>` 还是 `### v<version>`，与现有 changelog 标题层级对齐）
- [ ] 单版本归档是否同时镜像一份到 gh-pages 给文档站用（与 docs-site 的发布日志页联动 — 可以放到后续 ADR）
- [ ] 上线后观察 `truncated=true` 的命中率，若长期偏高再考虑把 `MAX_MERGE` 从 5 调到 10
