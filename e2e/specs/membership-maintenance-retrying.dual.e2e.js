import { dualDescribe } from '../helpers/dualPeer.js'
import { runMaintenanceScenario } from '../helpers/membershipMaintenance.js'

dualDescribe('成员维护暂时失败', () => {
  it('展示等待自动重试，并在真实回复后恢复正常', async () => {
    await runMaintenanceScenario('retryable')
  })
})
