// Optional published-runtime smoke runner for the lifecycle test. All paths are
// test-owned; the Podman protocol fixture never evaluates incoming SSH scripts.
const { spawn, execFile } = require('node:child_process');
const { promisify } = require('node:util');
const { openSync, closeSync, readFileSync } = require('node:fs');
const path = require('node:path');
const [binary, base, cwd] = process.argv.slice(2);
const logPath = path.join(base, 'native-smoke.log');
const log = openSync(logPath, 'w', 0o600);
const server = spawn(binary, ['serve', '--host', '127.0.0.1', '--port', '3773', '--base-dir', base, '--no-browser', '--auto-bootstrap-project-from-cwd'], {
  cwd, env: { ...process.env, HOME: path.dirname(base) }, stdio: ['ignore', log, log],
});
closeSync(log);
const exited = new Promise(resolve => server.once('exit', resolve));
const run = promisify(execFile);
async function verify() {
  try {
    await run(binary, ['__ssh-helper', 'wait-ready', '3773', '30000', '100'], { timeout: 35000 });
    const probe = path.resolve(__dirname, '../../src/cmd/update_agents_t3_terminal.js');
    const result = await run(process.execPath, [probe, binary, base, cwd], { timeout: 30000 });
    process.stdout.write(result.stdout);
  } finally {
    server.kill('SIGTERM');
    const deadline = setTimeout(() => server.kill('SIGKILL'), 5000);
    await exited;
    clearTimeout(deadline);
  }
}
verify().catch(error => {
  console.error(error.stderr || error.message);
  console.error(readFileSync(logPath, 'utf8'));
  process.exitCode = 1;
});
