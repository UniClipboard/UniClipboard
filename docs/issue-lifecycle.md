# Issue 验证流程

Issue 不会因为 PR 合并而自动关闭。每个状态的变更和关联的 PR、发布版本、验证记录都保留在 Issue 时间线中。

## 状态流转

```text
status:triage -> status:in-progress -> status:ready-for-test -> status:verified -> closed
```

- 创建和排查：`status:triage`
- 开始开发：`status:in-progress`
- PR 合并：自动变为 `status:ready-for-test`
- GitHub Release 发布：自动在每个待验证 Issue 留下版本和验证指令，包含自动发布的 Alpha 版本
- 验证通过：有权限的协作者评论验证指令，自动标记为 `status:verified` 并关闭

## 关联 PR

PR 描述中只能使用非关闭式关联，每条关联独占一行：

```markdown
## Related Issues

Related to #123
Refs #456
```

禁止使用 `Close`、`Closes`、`Fix`、`Fixes`、`Resolve`、`Resolves` 及其过去式来引用 Issue。这些写法会被 GitHub 当作合并后自动关闭的指令，工作流会阻止该 PR 通过检查。

在分支保护中，将 `Issue lifecycle / Validate issue references` 设为必需检查，才能强制执行这条规则。

## 验证并关闭

发布后，待验证 Issue 会收到类似下面的评论：

```text
Release v1.2.3 is available for verification.

After verification passes, an authorized collaborator can comment `/verify v1.2.3` to close this issue.
```

完成验证后，在同一 Issue 评论：

```text
/verify v1.2.3
```

只有拥有仓库分流、写入、维护或管理权限的协作者可以执行该指令。指令必须对应 Issue 已收到的发布版本，避免把尚未进入该版本的修复误标记为已验证。
