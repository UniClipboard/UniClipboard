import { appendFileSync, writeFileSync } from 'node:fs'
const ws = new WebSocket(process.env.E2E_WS_URL, {
  headers: { Authorization: `Session ${process.env.E2E_SESSION}` },
})
ws.onopen = () => ws.send(JSON.stringify({ action: 'subscribe', topics: ['settings'] }))
ws.onmessage = ({ data }) => {
  appendFileSync(process.env.E2E_EVENTS, data + '\n')
  if (JSON.parse(data).type === 'settings.snapshot') writeFileSync(process.env.E2E_READY, 'ready')
}
ws.onerror = () => process.exit(2)
process.on('SIGTERM', () => {
  ws.close()
  process.exit(0)
})
