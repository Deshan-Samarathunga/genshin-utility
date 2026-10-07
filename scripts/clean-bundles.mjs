// Called by semantic-release before building: removes installers from earlier builds so only the
// new version's files match the release upload patterns.
//
// A copy of the app started from the bundle folder (the portable exe) keeps that folder locked on
// Windows, so folders that can't be removed are emptied file by file instead, and a running exe is
// moved aside to target/release so it can't be uploaded with the new release.
import { existsSync, readdirSync, renameSync, rmSync, statSync } from 'node:fs';
import { join } from 'node:path';

const BUNDLE = 'tauri-gui/src-tauri/target/release/bundle';

function clear(path) {
  try {
    rmSync(path, { recursive: true, force: true });
    return;
  } catch {
    // Locked: fall through and remove what's inside.
  }
  if (!statSync(path).isDirectory()) {
    // A running exe can't be deleted but can be renamed out of the way.
    renameSync(path, join(BUNDLE, '..', `old-${Date.now()}-${path.split(/[\\/]/).pop()}`));
    return;
  }
  for (const entry of readdirSync(path)) clear(join(path, entry));
}

if (existsSync(BUNDLE)) clear(BUNDLE);
