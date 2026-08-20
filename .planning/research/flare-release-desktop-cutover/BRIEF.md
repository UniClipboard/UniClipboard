# Brief: Desktop 发布迁移到 FlareRelease

**日期：** 2026-08-20
**状态：** Locked
**实现日期：** 2026-08-20
**研究问题：** Desktop 发布工作流如何在不改变公开 URL 的前提下，把可变发布状态交给 FlareRelease？

## 建议

继续由 Desktop CI 把不可变安装包上传到现有 R2 路径，然后通过受 Cloudflare Access 保护的 FlareRelease Admin API 登记 Release。登记只产生 Ready Release，不自动 Promote；Channel 只能在 FlareRelease 中显式变更。

## 关键结论

1. Admin 地址为 `https://release-admin.uniclipboard.app`，登记接口为 `POST /api/releases/register`。
2. CI 使用独立的 Access service token，通过 `CF-Access-Client-Id` 和 `CF-Access-Client-Secret` 请求头认证。
3. Desktop 登记必须包含版本、tag、预发布标记、发布时间、双语说明，以及每个平台产物的 R2 key、公开 URL、大小和签名。
4. FlareRelease 登记会验证 R2 对象并保存不可变客户端响应，但不会修改 Channel。
5. `release.uniclipboard.app` 和 `/artifacts/v<version>/<filename>` 均保持不变。

## 实施方式

- 复用现有 updater manifest 组装脚本的平台识别结果，再转换为 FlareRelease 登记 JSON，避免维护第二套平台选择规则。
- release workflow 继续上传 R2 artifact，但停止写 R2 manifest、release-note index 和 GitHub Pages manifest。
- 删除旧 Worker 的部署 workflow，保留 `workers/update-server` 代码用于约定的回滚窗口。
- 删除只服务旧元数据写入方式的恢复 workflow。

## 不采用的方案

- CI 登记后自动 Promote：会让“构建完成”和“向用户发布”重新耦合，违反 FlareRelease 的控制边界。
- 在新脚本中重新扫描并判断平台：会复制 updater manifest 的平台规则，形成两个事实来源。
- 立即删除旧 Worker：生产验证和回滚窗口尚未完成，issue 明确要求保留。

## 约束

- Access 凭据由 `UniClipboard` GitHub 组织的 Actions secrets 统一管理，并授权给 Desktop 与 UniClip 仓库；凭据不写入仓库。
- 公开 updater 和 artifact URL 不变。
- R2 中旧的可变 JSON 作为迁移备份保留，但 CI 不再更新。
- registration 失败必须让发布 workflow 失败。

## 实施清单

- [x] 明确 FlareRelease API 与认证契约
- [x] 生成并验证 Desktop registration payload
- [x] 修改 release workflow
- [x] 停止旧 Worker 部署与旧恢复入口
- [x] 更新发布文档和 ADR
- [x] 运行脚本、测试、格式和 workflow 检查

## 生产切换状态

2026-08-20 检查时，`release.uniclipboard.app/health` 仍返回旧 update-server。FlareRelease Access service token 已改由 GitHub organization secrets 管理，维护者已确认 Desktop 与 UniClip 两个仓库都在授权范围内。仓库改动不得在完成 FlareRelease 导入、公开 Worker 切换与生产验证之前合入发布分支。
