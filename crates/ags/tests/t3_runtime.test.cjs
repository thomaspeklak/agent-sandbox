const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { prepareT3Runtime } = require('../src/cmd/update_agents_t3.js');
const { snapshot } = require('../src/cmd/update_agents_manifest.js');

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ags-t3-runtime-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const pnpm = path.join(root, 'pnpm-home');
  const install = path.join(pnpm, 'global/v11/physical-id/node_modules');
  const packageRoot = path.join(install, 't3');
  const name = `@t3code/t3-${process.platform}-${process.arch}`;
  const platform = path.join(packageRoot, 'node_modules', name);
  fs.mkdirSync(platform, { recursive: true });
  fs.writeFileSync(path.join(packageRoot, 'package.json'), JSON.stringify({ name: 't3', version: '0.0.45', optionalDependencies: { [name]: '0.0.45' } }));
  fs.writeFileSync(path.join(platform, 'package.json'), JSON.stringify({ name, version: '0.0.45' }));
  fs.writeFileSync(path.join(platform, 't3'), '#!/bin/sh\nprintf "0.0.45\\n"\n', { mode: 0o755 });
  for (const folder of ['client', 'resource-monitor', 'node_modules/native']) fs.mkdirSync(path.join(platform, folder), { recursive: true });
  fs.writeFileSync(path.join(platform, 'client/index.html'), 'web');
  fs.writeFileSync(path.join(platform, 'resource-monitor/monitor'), 'monitor');
  fs.writeFileSync(path.join(platform, 'node_modules/native/module.node'), 'native v1');
  fs.mkdirSync(path.join(pnpm, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(pnpm, 'bin/t3'), '#!/bin/sh\n# cmd-shim-target=/usr/local/pnpm/global/v11/physical-id/node_modules/t3/bin/t3.js\n');
  return { pnpm, packageRoot, platform, view: path.join(pnpm, 'ags-t3-runtime') };
}

test('pnpm bundle becomes an independent complete exact-version SSH runtime', t => {
  const data = fixture(t);
  const { destination } = prepareT3Runtime(data.packageRoot, data.view);
  assert.equal(fs.readFileSync(path.join(destination, '.install-complete'), 'utf8'), '0.0.45\n');
  assert.equal(fs.readFileSync(path.join(data.view, 'version'), 'utf8'), '0.0.45\n');
  fs.writeFileSync(path.join(data.platform, 'node_modules/native/module.node'), 'mutated install');
  assert.equal(fs.readFileSync(path.join(destination, 'node_modules/native/module.node'), 'utf8'), 'native v1');
  assert.equal(fs.readFileSync(path.join(destination, 'client/index.html'), 'utf8'), 'web');
});

test('T3-only inventory detects native, web, monitor, and compatibility changes', t => {
  const data = fixture(t);
  const { destination } = prepareT3Runtime(data.packageRoot, data.view);
  const inventory = () => snapshot(['t3'], { t3: { path: data.packageRoot } }, { 'pnpm-home': data.pnpm });
  const initial = inventory();
  assert(initial.some(entry => entry.key === 'launcher:t3'));
  assert(initial.some(entry => entry.key.includes('module.node')));
  for (const file of ['node_modules/native/module.node', 'client/index.html', 'resource-monitor/monitor', '.install-complete']) {
    const full = path.join(destination, file);
    const previous = fs.readFileSync(full);
    fs.appendFileSync(full, 'changed');
    assert.notDeepEqual(inventory(), initial, `change to ${file} must affect runtime identity`);
    fs.writeFileSync(full, previous);
  }
});

test('incomplete platform bundles and package/binary version mismatches cannot complete', t => {
  const data = fixture(t);
  fs.writeFileSync(path.join(data.platform, 't3'), '#!/bin/sh\nprintf "0.0.44\\n"\n', { mode: 0o755 });
  assert.throws(() => prepareT3Runtime(data.packageRoot, data.view), /expected 0.0.45/);
  assert(!fs.existsSync(path.join(data.view, 'versions/0.0.45/.install-complete')));
  fs.rmSync(path.join(data.platform, 'resource-monitor'), { recursive: true });
  assert.throws(() => prepareT3Runtime(data.packageRoot, data.view), /bundle lacks resource-monitor/);
});
