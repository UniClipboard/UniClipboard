import { Call } from '@wailsio/runtime'
import { useEffect, useState } from 'react'
import { daemonClient } from '@/api/daemon/client'
import { daemonWs } from '@/lib/daemon-ws'
import { connection, openSecondary, quit, type Connection } from './native'
import './style.css'

const windowName = new URLSearchParams(location.search).get('window') ?? 'main'
export default function App() {
  const [info, setInfo] = useState<Connection>()
  const [health, setHealth] = useState('等待连接')
  const [ws, setWS] = useState(false)
  const [session, setSession] = useState(false)
  const [refreshed, setRefreshed] = useState(false)
  const [error, setError] = useState('')
  const refresh = async () => {
    await daemonClient.refreshSession()
    await daemonClient.request('/settings')
    setRefreshed(true)
  }
  useEffect(() => {
    let active = true
    const unsubscribe = daemonWs.subscribe(['status'], event => {
      if (active && event.eventType === 'status.snapshot') setWS(true)
    })
    async function connect() {
      const config = await connection()
      if (!active) return
      setInfo(config)
      daemonClient.initialize(config)
      await daemonClient.refreshSession()
      await daemonClient.request('/settings')
      const result = await daemonClient.request<{
        data: { status: string; packageVersion: string }
      }>('/health')
      if (!active) return
      setSession(true)
      setHealth(`${result.data.status} · ${result.data.packageVersion}`)
      await daemonWs.connect(config.wsUrl)
    }
    void connect().catch(() => {
      if (active) setError('连接失败；请查看隔离环境的原生日志')
    })
    return () => {
      active = false
      unsubscribe()
      daemonWs.disconnect()
      daemonClient.destroy()
    }
  }, [])
  useEffect(() => {
    if (import.meta.env.VITE_E2E !== '1' || !info || !ws || !session || !refreshed) return
    void Call.ByName('main.EvidenceService.Record', {
      window: windowName,
      http: true,
      ws,
      session,
      refresh: refreshed,
      origin: location.origin,
      pid: info.pid,
    }).catch(() => setError('原生验收记录失败'))
  }, [info, ws, session, refreshed])
  useEffect(() => {
    if (import.meta.env.VITE_E2E !== '1' || !ws || !session) return
    document.getElementById('refresh')?.click()
    if (windowName === 'main') document.getElementById('secondary')?.click()
  }, [ws, session])
  return (
    <main>
      <header>
        <span className="badge">Wails v3 · beta.28</span>
        <h1>UniClipboard Go GUI</h1>
        <p>Rust daemon / Engine + Go 宿主 + React WebView</p>
      </header>
      <section aria-label="连接状态">
        <dl>
          <dt>窗口</dt>
          <dd>{windowName === 'main' ? '主窗口' : '第二窗口'}</dd>
          <dt>独立环境</dt>
          <dd>{info?.profile ?? '等待 native binding'}</dd>
          <dt>后台进程</dt>
          <dd>{info?.pid ?? '—'}</dd>
          <dt>HTTP 健康</dt>
          <dd id="health">{health}</dd>
          <dt>短期会话</dt>
          <dd>{session ? '已认证' : '等待认证'}</dd>
          <dt>WebSocket 快照</dt>
          <dd id="ws">{ws ? '已收到 status 快照' : '等待快照'}</dd>
          <dt>会话刷新</dt>
          <dd>{refreshed ? '刷新后 HTTP 请求成功' : '待验证'}</dd>
        </dl>
      </section>
      {error && <p role="alert">{error}</p>}
      <nav aria-label="原型操作">
        <button id="refresh" onClick={() => void refresh().catch(() => setError('会话刷新失败'))}>
          刷新认证与 HTTP
        </button>
        <button
          id="secondary"
          onClick={() => void openSecondary().catch(() => setError('第二窗口打开失败'))}
        >
          打开第二窗口
        </button>
        <button onClick={() => void quit().catch(() => setError('退出失败'))}>退出 GUI</button>
      </nav>
      <footer>
        隔离原型 · 禁用系统剪贴板 · 退出窗口后保留后台
        <br />
        复用现有 React HTTP / WebSocket 客户端；尚未迁移全部业务页面。
      </footer>
    </main>
  )
}
