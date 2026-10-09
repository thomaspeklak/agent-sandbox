// Build an independent SSH-compatible bundle inside the candidate generation.
// No runtime downloader or lifecycle script is involved.
const fs = require('node:fs');
const path = require('node:path');
const { createRequire } = require('node:module');
const { execFileSync } = require('node:child_process');
function prepareT3Runtime(packagePath, root) {
const packageRoot = fs.realpathSync(packagePath);
const manifest = JSON.parse(fs.readFileSync(path.join(packageRoot, 'package.json'), 'utf8'));
const version = manifest.version;
if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/.test(version)) {
  throw new Error('T3 package must contain an exact runtime version');
}
const resolve = createRequire(path.join(packageRoot, 'package.json'));
const platformName = `@t3code/t3-${process.platform}-${process.arch}`;
const platformRoot = path.dirname(resolve.resolve(`${platformName}/package.json`));
const platform = JSON.parse(fs.readFileSync(path.join(platformRoot, 'package.json'), 'utf8'));
if (platform.version !== version || manifest.optionalDependencies?.[platformName] !== version) {
  throw new Error('T3 launcher and platform bundle versions differ');
}
for (const entry of ['t3', 'client', 'resource-monitor', 'node_modules']) {
  if (!fs.existsSync(path.join(platformRoot, entry))) throw new Error(`T3 bundle lacks ${entry}`);
}
fs.rmSync(root, { recursive: true, force: true });
const destination = path.join(root, 'versions', version);
fs.mkdirSync(path.dirname(destination), { recursive: true });
// Dereference pnpm links so the compatibility bundle has no mutable store alias.
fs.cpSync(platformRoot, destination, { recursive: true, dereference: true });
const executable = path.join(destination, 't3');
const actual = execFileSync(executable, ['--version'], { encoding: 'utf8', timeout: 60000 }).trim();
if (actual.replace(/^(?:t3\s+)?v?/, '') !== version) throw new Error(`T3 binary reports ${actual}, expected ${version}`);
fs.writeFileSync(path.join(destination, '.install-complete'), `${version}\n`);
fs.writeFileSync(path.join(root, 'version'), `${version}\n`);
return { version, destination };
}
module.exports = { prepareT3Runtime };
// The updater executes this source through stdin, where require.main is unset.
if (require.main === module || module.id === '[stdin]') {
  if (!process.env.T3_PACKAGE_PATH) throw new Error('T3_PACKAGE_PATH is required to prepare the T3 runtime');
  prepareT3Runtime(process.env.T3_PACKAGE_PATH, '/usr/local/pnpm/ags-t3-runtime');
}
