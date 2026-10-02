import { dualDescribe } from '../helpers/dualPeer.js'
import { runMaintenanceScenario } from '../helpers/membershipMaintenance.js'

dualDescribe('成员维护需要处理', () => {
  it('展示检查方向，并在新连接机会的真实回复后恢复正常', async () => {
    await runMaintenanceScenario('needs_attention')
  })
})
