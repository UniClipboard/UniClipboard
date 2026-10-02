import { createServer } from 'node:http'

const tickets = new Map()
let sequence = 0

const server = createServer(async (request, response) => {
  const chunks = []
  for await (const chunk of request) chunks.push(chunk)
  let body
  try {
    body = JSON.parse(Buffer.concat(chunks).toString('utf8'))
  } catch {
    response.writeHead(400).end()
    return
  }
  const send = (status, value) => {
    response.writeHead(status, { 'Content-Type': 'application/json' })
    response.end(JSON.stringify(value))
  }
  if (request.method !== 'POST') return send(405, {})
  if (request.url === '/v1/pairings') {
    if (typeof body.sponsorTicket !== 'string') return send(400, {})
    const length = body.codeLength ?? 8
    if (length !== 6 && length !== 8) return send(400, {})
    const digits = String(++sequence).padStart(length, '0')
    const code = `${digits.slice(0, length / 2)}-${digits.slice(length / 2)}`
    tickets.set(code, body.sponsorTicket)
    return send(200, { code, expiresAtMs: 4102444800000 })
  }
  if (request.url === '/v1/pairings/resolve') {
    const code = body.code ?? body.pairingCode
    const sponsorTicket = tickets.get(code)
    return sponsorTicket
      ? send(200, { sponsorTicket, sponsorEndpointId: 'ignored', expiresAtMs: 4102444800000 })
      : send(404, {})
  }
  if (request.url === '/v1/pairings/consume') {
    tickets.delete(body.code ?? body.pairingCode)
    response.writeHead(204).end()
    return
  }
  send(404, {})
})

server.listen(0, '127.0.0.1', () => {
  process.stdout.write(`http://127.0.0.1:${server.address().port}\n`)
})
