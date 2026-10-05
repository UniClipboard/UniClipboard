# 历史标签布局（颜色 / 侧边栏常驻 / 侧边栏排序）

状态：**已实现**（2026-10-05）。尚未提交，也没有 PR。

## 决定

- **归属**：标签布局由产品侧后端（daemon，`crates/uc-webserver/src/api/tag_layout.rs`）管理，不进 `UniClipboard/Engine` 的标签模型。Engine 只拥有标签、加密后的名称和条目关联。
- **明文存储（已获批）**：布局以明文 JSON 存储，不做 MasterKey 加密。理由：
  - 文件里只有三类值：标签 id（4 个内置 id，或 Engine 生成的随机 UUIDv4，与名称无关）、颜色（5 个预设名之一，或自定义的 `#rrggbb`）、侧边栏顺序。
  - 拿到文件的人最多知道有几个标签、各是什么颜色、哪些在侧边栏以及顺序；拿不到标签名，也拿不到标签关联了哪些条目。
  - 产品侧拿不到 MasterKey（Engine 没有面向宿主的加密操作）；改用钥匙串里的独立密钥，在 `FileSecureStorage` 下密钥本身就是明文，形同未加密。
- **守护条件**：`TagLayoutId` 只接受内置 id 或 36 位 UUID；`HistoryTagColorDto` 只接受 5 个预设名或 `#rrggbb`；文档结构 `deny_unknown_fields`。测试 `ids_are_builtin_tags_or_uuids_and_nothing_else` 和 `a_stored_layout_with_anything_but_ids_is_refused` 锁住这一点。将来要加名称、备注等自由文本字段，必须重新讨论加密。
- **默认侧边栏**：`link / code / image / directory` 四个内置标签。`favorited` 只通过 Pinned 行出现，不进布局；`file` 不是标签。
- **新建标签不自动进侧边栏**；侧边栏显示布局顺序的前 6 个。

## 存储

- 路径：`<app_data_root>/history-tags/layout.v1.json`，在 `apps/daemon/src/daemon/host.rs` 注入。
- 结构：`{ "version": 1, "sidebar": [id…], "colors": { id: color } }`。`colors` 只存用户选过的颜色；内置标签的默认颜色（link 蓝、code 紫、image 绿、directory 灰）在读出时补上。
- 写入：同目录临时文件（`tempfile`，权限 `0o600`）→ `sync_all` → `rename`。先写盘成功才更新内存，写失败时布局保持原样。
- 单写者：`TagLayoutStore` 内一把 `tokio::sync::Mutex`；daemon 有实例锁，不会多进程并发写。
- 读失败（损坏、版本不对）：记录 `warn`，按默认布局提供服务；只读不覆盖原文件，下一次修改时才写入新文件。

## API

| 路由 | 行为 |
| --- | --- |
| `GET /history/tags/layout` | 返回 `{ sidebar, colors }`；同时清掉 Engine 已不存在的本地标签 id。 |
| `PUT /history/tags/layout/sidebar` | `{ tagIds }` 整体替换侧边栏顺序，重复项合并。 |
| `PUT /history/tags/{tag_id}/color` | `{ color }`，`null` 清除（内置标签回到默认色）。 |
| `PUT /history/tags/{tag_id}/sidebar` | `{ inSidebar }`，加入时排到最后。 |
| `POST /history/tags` | 可选 `color`，只对新建的标签生效；同名已存在时不改颜色。 |
| `DELETE /history/tags/{tag_id}` | Engine 删除成功后从布局移除。 |
| `POST /history/tags/{tag_id}/merge` | 合并成功后移除来源；目标不在侧边栏时继承第一个来源的位置，没有颜色时继承来源的颜色。 |

- 布局路由先向 Engine 取当前标签列表：会话锁定时与标签接口一样返回 423；不存在的本地 id 返回 404；不能放进布局的 id（如 `favorited`、自由文本）返回 400。
- 删除和合并时的布局更新失败只记录日志，不影响 Engine 的结果；下一次 `GET` 会清理残留。

## 前端

- `useTagCatalog` 增加 `layout`；`TagColorsContext`（`apps/gui/src/components/history/tags/tag-colors-context.ts`）把颜色提供给所有标签界面；`tagTint(color)`（`apps/gui/src/lib/tag-colors.ts`）取代了按 id 哈希的配色。
- 侧边栏按布局顺序显示前 6 个，内置标签和本地标签都可以出现。
- 标签编辑器的 Create 行带五色色板，⇥ / ⇧⇥ 循环切换；默认橙色。搜索框的 `+ Create`、Library 的新建也用橙色。
- Library 列出 4 个内置标签和全部本地标签；IN SIDEBAR 开关、行菜单里的 Color 对两者都可用；内置标签不能改名、合并、删除或勾选。

## 未做

- 侧边栏顺序的调整入口（拖拽或 Library 内排序）设计稿未给出，接口 `PUT /history/tags/layout/sidebar` 已就绪。
- `unused · 90 days` 需要记录最后使用时间，当前仍按“条目数为 0”判断未使用。
