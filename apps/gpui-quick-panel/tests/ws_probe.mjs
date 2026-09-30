// Connects to the daemon's WebSocket like a client of the given type would, subscribes to the
// given topics and prints every frame it receives as one JSON line, until the time is up.
//
// Usage: node ws_probe.mjs <daemon.conn path> <clientType> <pid> <seconds> <topic,topic,...>
import { readFileSync } from 'node:fs';

const [connPath, clientType, pid, seconds, topics] = process.argv.slice(2);
const conn = JSON.parse(readFileSync(connPath, 'utf8'));
const base = `http://${conn.host}:${conn.port}`;

const response = await fetch(`${base}/auth/connect`, {
  method: 'POST',
  headers: { 'content-type': 'application/json', authorization: `Bearer ${conn.token}` },
  body: JSON.stringify({ pid: Number(pid), clientType }),
});
const { data } = await response.json();
const socket = new WebSocket(`ws://${conn.host}:${conn.port}/ws?auth=${encodeURIComponent(`Session ${data.sessionToken}`)}`);
socket.addEventListener('open', () => {
  socket.send(JSON.stringify({ action: 'subscribe', topics: topics.split(','), nonce: 'probe' }));
  console.log(JSON.stringify({ probe: 'open', clientType, ts: Date.now() }));
});
socket.addEventListener('message', event => {
  console.log(JSON.stringify({ probe: 'frame', ts: Date.now(), frame: String(event.data) }));
});
socket.addEventListener('error', () => console.log(JSON.stringify({ probe: 'error', ts: Date.now() })));
setTimeout(() => { console.log(JSON.stringify({ probe: 'done', ts: Date.now() })); process.exit(0); }, Number(seconds) * 1000);
