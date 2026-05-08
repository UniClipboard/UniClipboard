import React, { useEffect } from 'react'
import { refreshPresence } from '@/api/daemon'
import { MobileSyncDevicesPanel, SpaceMembersPanel, ThisDeviceCard } from '@/components'
import { ScrollArea } from '@/components/ui/scroll-area'
import { createLogger } from '@/lib/logger'
import { useAppDispatch } from '@/store/hooks'
import { fetchLocalDeviceInfo, fetchSpaceMembers } from '@/store/slices/devicesSlice'

const log = createLogger('devices-page')

// 主动 probe 间隔。QUIC `max_idle_timeout = 60s` 决定 watchdog 被动检测
// 离线的上限；这里每 15s 主动跑一轮 ensure_reachable_all，离线 peer 拨号
// 失败立即 broadcast(Offline) → peers.changed → UI 切灰，把"对端断网"
// 的反馈时延压到 ~15s + 拨号失败时间。页面隐藏时浏览器会自动节流定时器
// （后台 tab ≥ 1s），不再单独处理 visibility。
const PRESENCE_REFRESH_INTERVAL_MS = 15_000

const DevicesPage: React.FC = () => {
  const dispatch = useAppDispatch()

  useEffect(() => {
    dispatch(fetchLocalDeviceInfo())
    dispatch(fetchSpaceMembers())

    const probe = () => {
      refreshPresence().catch(err => {
        // setup 未完成 / daemon 未就绪时 refresh_presence 会 5xx；
        // 不影响后续 tick，warn 即可。
        log.warn({ err }, 'presence refresh failed')
      })
    }
    probe()
    const intervalId = setInterval(probe, PRESENCE_REFRESH_INTERVAL_MS)
    return () => clearInterval(intervalId)
  }, [dispatch])

  return (
    <div className="flex flex-col h-full relative">
      <div className="flex-1 overflow-hidden relative">
        <ScrollArea className="h-full">
          <div className="space-y-6 px-6 pb-10 pt-8">
            <ThisDeviceCard />
            <SpaceMembersPanel />
            <MobileSyncDevicesPanel />
          </div>
        </ScrollArea>
      </div>
    </div>
  )
}

export default DevicesPage
