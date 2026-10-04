// Called by semantic-release before building: removes installers from earlier builds so only the
// new version's files match the release upload patterns.
import { rmSync } from 'node:fs';

rmSync('tauri-gui/src-tauri/target/release/bundle', { recursive: true, force: true });
