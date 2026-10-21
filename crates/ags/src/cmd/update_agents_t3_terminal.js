// v0.0.45 HTTP auth + Effect JSON WebSocket contract, exercised inside the
// updater's offline, read-only verification container. No provider credentials.
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const [binary, base, cwd] = process.argv.slice(2);
const origin = 'http://127.0.0.1:3773';
const pending = new Map();
let sequence = 0;

async function jsonRequest(url, options) {
  const response = await fetch(`${origin}${url}`, { ...options, signal: AbortSignal.timeout(10000) });
  if (!response.ok) throw new Error(`T3 native verification HTTP ${url}: ${response.status}`);
  return response.json();
}

async function verify() {
  const pairing = JSON.parse(execFileSync(binary, ['auth', 'pairing', 'create', '--base-dir', base, '--json'], { encoding: 'utf8', timeout: 10000 }));
  const token = await jsonRequest('/oauth/token', {
    method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ grant_type: 'urn:ietf:params:oauth:grant-type:token-exchange',
      subject_token: pairing.credential, subject_token_type: 'urn:t3:params:oauth:token-type:environment-bootstrap',
      requested_token_type: 'urn:ietf:params:oauth:token-type:access_token' }),
  });
  const ticket = await jsonRequest('/api/auth/websocket-ticket', {
    method: 'POST', headers: { authorization: `Bearer ${token.access_token}` },
  });
  const socket = new WebSocket(`ws://127.0.0.1:3773/ws?wsTicket=${encodeURIComponent(ticket.ticket)}`);
  socket.addEventListener('message', event => {
    const decoded = JSON.parse(String(event.data));
    for (const message of Array.isArray(decoded) ? decoded : [decoded]) {
      if (message._tag === 'Ping') { socket.send(JSON.stringify({ _tag: 'Pong' })); continue; }
      if (message._tag !== 'Exit') continue;
      const waiter = pending.get(String(message.requestId));
      if (!waiter) continue;
      pending.delete(String(message.requestId));
      if (message.exit._tag === 'Success') waiter.resolve(message.exit.value);
      else waiter.reject(new Error(`T3 native RPC failed: ${JSON.stringify(message.exit)}`));
    }
  });
  const rpc = (tag, payload) => new Promise((resolve, reject) => {
    const id = String(++sequence);
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ _tag: 'Request', id, tag, payload, headers: [] }));
  });
  await new Promise((resolve, reject) => {
    socket.addEventListener('open', resolve, { once: true });
    socket.addEventListener('error', () => reject(new Error('T3 native verification WebSocket failed')), { once: true });
  });
  const browseRoot = fs.mkdtempSync(path.join(cwd, 'ags-browse-'));
  fs.mkdirSync(path.join(browseRoot, 'child directory'));
  fs.writeFileSync(path.join(browseRoot, 'regular-file'), 'not a directory');
  try {
    const config = await rpc('server.getConfig', {});
    if (config.settings.addProjectBaseDirectory !== cwd) throw new Error('T3 folder picker is not rooted in the mounted checkout');
    const root = await rpc('filesystem.browse', { partialPath: `${config.settings.addProjectBaseDirectory}/` });
    if (!root.entries.some(entry => entry.fullPath === browseRoot)) throw new Error('T3 folder picker cannot discover checkout directories');
    const prefix = await rpc('filesystem.browse', { partialPath: `${browseRoot}/chi` });
    if (prefix.entries.length !== 1 || prefix.entries[0].fullPath !== path.join(browseRoot, 'child directory')) throw new Error('T3 folder prefix autocomplete failed');
    const contents = await rpc('filesystem.browse', { partialPath: `${browseRoot}/` });
    if (contents.entries.some(entry => entry.name === 'regular-file')) throw new Error('T3 folder picker returned a regular file');
    const input = { threadId: 'ags-runtime-verification', terminalId: 'term-1', cwd, cols: 80, rows: 24 };
    const terminal = await rpc('terminal.open', input);
    if (terminal.status !== 'running' || !Number.isInteger(terminal.pid)) throw new Error('T3 native PTY did not start');
    await rpc('terminal.write', { threadId: input.threadId, terminalId: input.terminalId, data: "printf 'ags-native-ready\\n'\n" });
    await rpc('terminal.resize', { threadId: input.threadId, terminalId: input.terminalId, cols: 100, rows: 30 });
    await rpc('terminal.close', { threadId: input.threadId, terminalId: input.terminalId, deleteHistory: true });
  } finally { socket.close(); fs.rmSync(browseRoot, { recursive: true, force: true }); }
}

const deadline = setTimeout(() => { console.error('T3 native filesystem/terminal verification timed out'); process.exit(1); }, 20000);
verify().then(() => clearTimeout(deadline), error => {
  console.error(error.message); process.exit(1);
});
