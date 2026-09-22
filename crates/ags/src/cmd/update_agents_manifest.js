// Inventory executable/runtime files, not installer caches, logs, or timestamps.
// pnpm 11 uses random physical installation IDs: retain physical paths for sharing,
// but normalize those IDs in the semantic fingerprint and generated shell shims.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

function snapshot(agents, dependencies, roots) {
  const entries = [];
  const hashes = new Map();
  const normalize = text => text.replace(/\/usr\/local\/pnpm\/global\/v\d+\/[^/\s"']+/g, '$PNPM_INSTALL');
  function storage(file) {
    for (const [name, root] of Object.entries(roots)) {
      const rel = path.relative(root, file);
      if (rel && !rel.startsWith('../') && !path.isAbsolute(rel)) return `${name}/${rel}`;
    }
    throw new Error(`runtime file escapes managed mounts: ${file}`);
  }
  function hashFile(file, stat) {
    const key = `${stat.dev}:${stat.ino}`;
    if (hashes.has(key)) return hashes.get(key);
    const hash = crypto.createHash('sha256');
    const fd = fs.openSync(file, 'r');
    const buffer = Buffer.alloc(1024 * 1024);
    try {
      let count;
      while ((count = fs.readSync(fd, buffer, 0, buffer.length, null)) > 0) hash.update(buffer.subarray(0, count));
    } finally { fs.closeSync(fd); }
    const result = hash.digest('hex');
    hashes.set(key, result);
    return result;
  }
  function add(file, key, shim = false) {
    const stat = fs.lstatSync(file);
    const entry = { key, path: storage(file), mode: stat.mode & 0o7777, size: stat.size };
    if (stat.isSymbolicLink()) {
      entry.kind = 'link';
      entry.raw = fs.readlinkSync(file);
      entry.digest = normalize(entry.raw);
    } else if (stat.isDirectory()) {
      entry.kind = 'directory';
      entry.raw = entry.digest = '';
    } else if (stat.isFile()) {
      entry.kind = 'file';
      entry.raw = entry.digest = hashFile(file, stat);
      if (shim) {
        if (stat.size > 1024 * 1024) throw new Error(`oversized pnpm shim: ${file}`);
        entry.digest = crypto.createHash('sha256').update(normalize(fs.readFileSync(file, 'utf8'))).digest('hex');
      }
    } else { throw new Error(`unsupported runtime file: ${file}`); }
    entries.push(entry);
    return stat;
  }
  function tree(root, key, pnpm = false, relative = '') {
    const file = path.join(root, relative);
    // .modules.yaml contains install timestamps and physical layout bookkeeping.
    // The resolved lockfile, dependency files, and actual symlink layout are inventoried.
    if (pnpm && relative === '.modules.yaml') return;
    const stat = add(file, `${key}/${relative}`, pnpm && relative.split('/').includes('.bin'));
    if (pnpm && stat.isSymbolicLink()) {
      const target = path.relative(root, fs.realpathSync(file));
      if (target.startsWith('../') || path.isAbsolute(target)) {
        throw new Error(`pnpm dependency escapes isolated installation: ${file}`);
      }
    }
    if (stat.isDirectory()) {
      for (const name of fs.readdirSync(file).sort()) tree(root, key, pnpm, path.join(relative, name));
    }
  }
  if (agents.some(agent => agent === 'pi' || agent === 'gemini')) {
    for (const [name, dependency] of Object.entries(dependencies).sort()) {
      const marker = dependency.path.indexOf('/node_modules/');
      if (marker < 0) throw new Error(`unexpected pnpm dependency path for ${name}`);
      const install = fs.realpathSync(dependency.path.slice(0, marker));
      tree(path.join(install, 'node_modules'), `pnpm:${name}`, true);
    }
    for (const agent of agents.filter(agent => agent === 'pi' || agent === 'gemini')) {
      add(path.join(roots['pnpm-home'], 'bin', agent), `launcher:${agent}`, true);
    }
  }
  if (agents.includes('codex')) {
    tree(fs.realpathSync(path.join(roots['codex-install'], 'packages/standalone/current')), 'codex:release');
    add(path.join(roots['pnpm-home'], 'codex'), 'launcher:codex');
    const sidecar = path.join(roots['pnpm-home'], 'codex-code-mode-host');
    if (fs.existsSync(sidecar)) add(sidecar, 'launcher:codex-code-mode-host');
  }
  if (agents.includes('claude')) {
    const launcher = path.join(roots['claude-install'], '.local/bin/claude');
    add(launcher, 'launcher:claude');
    add(fs.realpathSync(launcher), 'claude:binary');
    add(path.join(roots['pnpm-home'], 'claude'), 'launcher:claude-wrapper');
  }
  if (agents.includes('opencode')) tree(path.join(roots['opencode-install'], '.opencode/bin'), 'opencode:bin');
  return entries.sort((a, b) => a.key < b.key ? -1 : a.key > b.key ? 1 : 0);
}
module.exports = { snapshot };
