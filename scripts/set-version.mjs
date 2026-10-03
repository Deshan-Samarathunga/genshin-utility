// Called by semantic-release: writes the release version into every place the app declares it.
import { readFileSync, writeFileSync } from 'node:fs';

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+/.test(version ?? '')) {
  console.error('usage: node scripts/set-version.mjs <version>');
  process.exit(1);
}

const updateJson = (path) => {
  const json = JSON.parse(readFileSync(path, 'utf8'));
  json.version = version;
  writeFileSync(path, `${JSON.stringify(json, null, 2)}\n`);
};

updateJson('tauri-gui/package.json');
updateJson('tauri-gui/src-tauri/tauri.conf.json');

// Only the [package] version, not dependency versions.
const cargoPath = 'tauri-gui/src-tauri/Cargo.toml';
const cargo = readFileSync(cargoPath, 'utf8');
const updated = cargo.replace(/(\[package\][^[]*?\nversion\s*=\s*")[^"]+(")/, `$1${version}$2`);
if (updated === cargo && !cargo.includes(`version = "${version}"`)) {
  console.error('Could not find the [package] version in Cargo.toml');
  process.exit(1);
}
writeFileSync(cargoPath, updated);

console.log(`Version set to ${version}`);
