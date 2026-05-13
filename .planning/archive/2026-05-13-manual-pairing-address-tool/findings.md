# 发现记录：开发者手动选择网卡配对工具

## 初始判断

- 工具应保持隐藏 / dev-only，避免改变正式用户的自动配对逻辑。
- 最小产品形态：先做 sponsor 侧，列出地址并用指定 IP 生成只包含该地址的配对码；join 侧继续复用现有 `join` 流程。
- 不能让 CLI 直接绕过业务流程；优先复用现有 app facade 和 infra adapter。

## 代码复核

- `uc-cli` 的业务命令必须通过 `AppFacade`，不能直接读 infra / iroh endpoint。
- 当前正常 `invite` 调用链是 `uniclip invite` → `AppFacade::issue_pairing_invitation` → `SpaceSetupFacade` → `IssuePairingInvitationUseCase` → `PairingInvitationPort`。
- `RendezvousPairingInvitationAdapter` 已有单一 ticket 过滤入口 `serialize_filtered_endpoint_ticket`，新增“指定 IP”应复用该过滤结果后再收窄，避免与 OnlyLan/虚拟网卡规则分叉。
- `PairingInvitationPort` 目前只负责签发/消费邀请；新增“列出可发布地址”和“按指定地址签发”属于同一 sponsor-side invitation 能力，但需要保持 core 文档只描述领域语义，不写 CLI/iroh/rendezvous。
