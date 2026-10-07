// Called by semantic-release: builds the signed installers and writes latest.json, the manifest the
// in-app updater downloads from the newest GitHub release.
//
// Signing key: TAURI_SIGNING_PRIVATE_KEY (key contents, used in CI) or the local key file at
// ~/.tauri/genshin-utility.key. Keep that file private and backed up; without it, installed copies
// can't verify (and won't accept) new versions.
import { execSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

const REPO = 'Deshan-Samarathunga/genshin-utility';
const BUNDLE = 'tauri-gui/src-tauri/target/release/bundle';

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+/.test(version ?? '')) {
  console.error('usage: node scripts/build-release.mjs <version>');
  process.exit(1);
}

const env = { ...process.env };
if (!env.TAURI_SIGNING_PRIVATE_KEY) {
  const keyFile = join(homedir(), '.tauri', 'genshin-utility.key');
  if (!existsSync(keyFile)) {
    console.error(`No updater signing key: set TAURI_SIGNING_PRIVATE_KEY or create ${keyFile}`);
    process.exit(1);
  }
  env.TAURI_SIGNING_PRIVATE_KEY = readFileSync(keyFile, 'utf8');
}
env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ??= '';

execSync('npm --prefix tauri-gui run tauri build', { stdio: 'inherit', env });

// GitHub stores uploaded asset names with spaces turned into dots.
const assetUrl = (file) =>
  `https://github.com/${REPO}/releases/download/v${version}/${encodeURIComponent(file.replaceAll(' ', '.'))}`;

const platform = (dir, suffix) => {
  const file = readdirSync(join(BUNDLE, dir)).find((f) => f.endsWith(suffix) && f.includes(version));
  if (!file) throw new Error(`No ${suffix} for ${version} in ${dir}`);
  return {
    signature: readFileSync(join(BUNDLE, dir, `${file}.sig`), 'utf8').trim(),
    url: assetUrl(file),
  };
};

const nsis = platform('nsis', '-setup.exe');
const manifest = {
  version,
  notes: `See https://github.com/${REPO}/releases/tag/v${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': nsis,
    'windows-x86_64-nsis': nsis,
    'windows-x86_64-msi': platform('msi', '.msi'),
  },
};
writeFileSync(join(BUNDLE, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Wrote ${BUNDLE}/latest.json`);

// Portable copy: the app is a single exe (the page is built in, settings live in %APPDATA%).
// "portable" in the file name tells the app to update by download instead of running the installer.
const portableDir = join(BUNDLE, 'portable');
mkdirSync(portableDir, { recursive: true });
const portable = join(portableDir, `Genshin Impact Utility_${version}_x64-portable.exe`);
copyFileSync('tauri-gui/src-tauri/target/release/Genshin Impact Utility.exe', portable);
console.log(`Wrote ${portable}`);
