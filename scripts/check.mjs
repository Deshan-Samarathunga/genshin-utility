// Pre-commit check: the frontend JS parses and the Rust backend compiles.
// Fast because both are incremental; output only appears when something fails.
// Skip with `git commit --no-verify` in an emergency.
import { execSync } from 'node:child_process';
import { readdirSync } from 'node:fs';
import { join } from 'node:path';

function run(label, cmd, cwd) {
  try {
    execSync(cmd, { cwd, stdio: 'pipe' });
  } catch (error) {
    console.error(`✖ ${label} failed\n`);
    console.error(`${error.stdout ?? ''}${error.stderr ?? ''}`);
    process.exit(1);
  }
}

const frontend = 'tauri-gui/src';
for (const file of readdirSync(frontend).filter((f) => f.endsWith('.js'))) {
  run(`JS syntax (${file})`, `node --check ${join(frontend, file)}`);
}
run('cargo check', 'cargo check --quiet', 'tauri-gui/src-tauri');
console.log('✔ pre-commit checks passed');
