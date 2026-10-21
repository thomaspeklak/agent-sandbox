const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const net = require('node:net');
const { once } = require('node:events');
const { spawn } = require('node:child_process');

const source = fs.readFileSync(path.join(__dirname, '../src/t3/forward-tcp.js'), 'utf8');

function relay(t, port) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ags-t3-forwarding-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const script = path.join(root, 'forward-tcp.js');
  // Use an OS-assigned port so independent test processes never share port 3773.
  assert.match(source, /port = 3773/);
  fs.writeFileSync(script, source.replace('port = 3773', `port = ${port}`));
  const child = spawn(process.execPath, [script], {
    // Only Node itself is available; the relay cannot invoke optional utilities.
    env: { ...process.env, PATH: root },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  t.after(() => child.kill());
  const stdout = [];
  const stderr = [];
  let stdoutBytes = 0;
  child.stdout.on('data', chunk => {
    stdout.push(chunk);
    stdoutBytes += chunk.length;
  });
  child.stderr.on('data', chunk => stderr.push(chunk));
  const closed = once(child, 'close');
  return {
    child,
    async read(length) {
      while (stdoutBytes < length) {
        assert(!child.stdout.readableEnded, 'relay ended before the expected bytes arrived');
        await once(child.stdout, 'data');
      }
      return Buffer.concat(stdout).subarray(0, length);
    },
    async result() {
      const [code, signal] = await closed;
      return { code, signal, stdout: Buffer.concat(stdout), stderr: Buffer.concat(stderr).toString() };
    },
  };
}

async function server(t, handler) {
  const sockets = new Set();
  const listener = net.createServer({ allowHalfOpen: true }, socket => {
    sockets.add(socket);
    socket.on('error', () => {});
    socket.on('close', () => sockets.delete(socket));
    handler(socket);
  });
  listener.listen(0, '127.0.0.1');
  await once(listener, 'listening');
  t.after(async () => {
    for (const socket of sockets) socket.destroy();
    await new Promise(resolve => listener.close(resolve));
  });
  return listener.address().port;
}

test('relay streams both ways before EOF and receives a reply after TCP half-close', { timeout: 10000 }, async t => {
  const request = Buffer.from([0, 255, 1, 128, 10]);
  const received = [];
  const port = await server(t, socket => {
    socket.write('greeting');
    socket.on('data', chunk => {
      received.push(chunk);
      socket.write(chunk);
    });
    socket.on('end', () => socket.end('after EOF'));
  });
  const run = relay(t, port);
  assert.deepEqual(await run.read(8), Buffer.from('greeting'));
  run.child.stdin.write(request);
  assert.deepEqual(await run.read(8 + request.length), Buffer.concat([Buffer.from('greeting'), request]));
  run.child.stdin.end();
  const result = await run.result();
  assert.equal(result.code, 0, result.stderr);
  assert.deepEqual(Buffer.concat(received), request);
  assert.deepEqual(result.stdout, Buffer.concat([Buffer.from('greeting'), request, Buffer.from('after EOF')]));
});

test('relay preserves large binary transfers under pipe backpressure', { timeout: 10000 }, async t => {
  const payload = Buffer.alloc(2 * 1024 * 1024);
  for (let index = 0; index < payload.length; index++) payload[index] = index % 251;
  const received = [];
  const port = await server(t, socket => {
    socket.on('data', chunk => received.push(chunk));
    socket.on('end', () => socket.end(Buffer.concat([Buffer.from('reply:'), ...received])));
  });
  const run = relay(t, port);
  run.child.stdout.pause();
  assert.equal(run.child.stdin.write(payload), false, 'a large request must apply backpressure');
  run.child.stdin.end();
  const timer = setTimeout(() => run.child.stdout.resume(), 50);
  t.after(() => clearTimeout(timer));
  const result = await run.result();
  assert.equal(result.code, 0, result.stderr);
  assert.deepEqual(Buffer.concat(received), payload);
  assert.deepEqual(result.stdout, Buffer.concat([Buffer.from('reply:'), payload]));
});

test('server EOF leaves the stdin direction open until its own EOF', { timeout: 10000 }, async t => {
  const received = [];
  const port = await server(t, socket => {
    socket.end('server EOF');
    socket.on('data', chunk => received.push(chunk));
    socket.on('end', () => socket.destroy());
  });
  const run = relay(t, port);
  await once(run.child.stdout, 'end');
  run.child.stdin.end('request after server EOF');
  const result = await run.result();
  assert.equal(result.code, 0, result.stderr);
  assert.equal(result.stdout.toString(), 'server EOF');
  assert.equal(Buffer.concat(received).toString(), 'request after server EOF');
});

test('relay fails promptly when the loopback server refuses the connection', { timeout: 10000 }, async t => {
  const listener = net.createServer();
  listener.listen(0, '127.0.0.1');
  await once(listener, 'listening');
  const port = listener.address().port;
  await new Promise(resolve => listener.close(resolve));
  const run = relay(t, port);
  const result = await run.result();
  assert.equal(result.code, 1);
  assert.equal(result.stdout.length, 0);
  assert.match(result.stderr, /AGS T3 forwarding failed:.*ECONNREFUSED/);
});

test('cancelling the relay closes its TCP connection', { timeout: 10000 }, async t => {
  let disconnected;
  const port = await server(t, socket => {
    disconnected = once(socket, 'close');
    socket.on('end', () => socket.end());
    socket.write('connected');
  });
  const run = relay(t, port);
  await run.read(9);
  run.child.kill('SIGTERM');
  const result = await run.result();
  assert.equal(result.signal, 'SIGTERM');
  await disconnected;
});
