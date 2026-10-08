# Genshin Impact Utility (Tauri)

A modern, fast, and lightweight frontend for Genshin Impact AutoHotkey scripts, built with Tauri v2, Rust, and Vanilla web technologies.

## Prerequisites

To run this application, you need to install the following dependencies on your Windows machine:

1. **AutoHotkey v2**: 
   - Download and install from [autohotkey.com](https://www.autohotkey.com/) (Make sure it's v2).
2. **Node.js**:
   - Download and install from [nodejs.org](https://nodejs.org/).
3. **Rust Build Tools**:
   - Download and install [rustup](https://rustup.rs/).
   - Install the **"Desktop development with C++"** workload via the Visual Studio Installer (Required for compiling Tauri apps on Windows).

## Running the Application (Development)

To run the application in development mode with hot-reloading:

1. Open a terminal (Command Prompt or PowerShell).
2. Navigate to the `tauri-gui` directory:
   ```bash
   cd path\to\genshin-autohotkey\tauri-gui
   ```
3. Install the Node dependencies:
   ```bash
   npm install
   ```
4. Start the Tauri development server:
   ```bash
   npm run tauri dev
   ```

*Note: The first time you run `npm run tauri dev`, it will take a few minutes to download and compile the Rust crates. Subsequent runs will be much faster.*

## Building for Production

To create a standalone `.exe` installer for the application:

```bash
npm run tauri build
```

The compiled installer will be located in `src-tauri/target/release/bundle/`.

**Portable version:** `bundle/portable/*-portable.exe` is the whole app in one file — run it from any folder, no install. Settings are stored in `%APPDATA%\com.frost.genshinutility` either way, so the portable and installed copies share them. Keep "portable" in the file name: the app uses it to update by download instead of running the installer.

Builds are signed for the in-app updater, so `tauri build` needs the private key: set `TAURI_SIGNING_PRIVATE_KEY` (key contents) or use `node scripts/build-release.mjs <version>` from the repo root, which reads `~/.tauri/genshin-utility.key`.

## Story Mode

A separate tab next to Auto Dialogue (only one of the two is on at a time; both use **F4** in-game to start and stop by default). It skips dialogue like Auto Dialogue, with its own delay (with *Longer lines get more time* on, the delay is for an average line: short lines go faster, long ones get up to 3x), and also:

- reads the subtitles off the screen with Windows' built-in OCR (offline, no cost per line, keeps up at any delay),
- asks an AI which option to pick when a choice menu appears (instead of always the first one),
- writes a summary of the whole conversation when you stop, shown in the Story Mode tab with a Copy button.

Every run is kept in **History** (newest first, with a title the AI gives it). If you play a story over several sittings, tick those sessions and press **Merge** for one summary of the whole thing, in play order. Sessions that share character and place names are marked as related: click *N related* to tick them all, or tick one to see its related sessions highlighted. History is included in settings export/import. *Summary* sets the length: **Very detailed** (default) writes up every part of a long story scene by scene (about 120 lines per part, so free AI tiers can handle it) under an overview, with Characters, Lore & reveals and Your choices; **Detailed** is one long recap; **Short** is a few paragraphs. *Summarize again* rewrites a saved summary with the current setting. Every line is also saved as "Speaker: what they said" (the speaker is the gold name above the subtitle): open a session and press **Dialogue** to read the whole conversation with your choices and sitting breaks, and search it by line or name.

Nothing read is lost: the run in progress is written to disk as each line is read, so a crash, a closed app or a lost connection keeps it (it's recovered into History on the next start). Runs that couldn't be summarized (offline) are saved anyway and summarized automatically once the AI is reachable, or with *Retry summary*. If the game sends you back to a save point, press **Continue this story** on the session: your next runs are added to it, and lines you already read are skipped when you replay the scene (merging skips repeats too).

The AI uses the key and base URL from the **API Keys** tab, with the provider and model set in Story Mode (type any model the provider offers) (Groq, Gemini, OpenAI, Mistral or a custom OpenAI-compatible server). If a call fails, the first option is picked as before. Options are recognised by the speech-bubble icon beside them, and only while a subtitle is on screen. Unlike Auto Dialogue it only presses Space when the small gold diamond at the bottom centre shows a line is finished, so it never presses Space outside a conversation (no jumping), and open-world chatter (HUD showing) is recorded but never skipped with Space: in the open world it only presses F once to start talking to someone shown in a speech-bubble prompt (each NPC once per run, so finished chats are not restarted). It never shops: if a menu with *Purchase / Buy / Exchange / Craft / Confirm* is open it pauses until you close it, and options that open a shop or trade are never picked. Built for the 16:9 layout in borderless/windowed mode.

## Voice Chat (PS5 controller or keyboard & mouse)

Type to friends in Genshin's chat by voice: from the couch with a DualSense, or at the desk with a
headset and a push-to-talk key.

1. **Controller:** USB or Bluetooth, with Genshin's native controller support (not Steam Input / DS4Windows). Tap the **PS** button to start talking and tap it again to finish (don't hold it: a long PS press is the controller's own shortcut and can disconnect it over Bluetooth). The controller's own mic only works over USB; on Bluetooth, pick your headset as the *Controller mic* in Voice Chat → Controls & mics. The mic is only opened while you hold the button, so Bluetooth headsets stay in their normal audio mode the rest of the time.
   **Keyboard & mouse:** pick your push-to-talk key in *Settings* (Mouse 4 by default; Mouse 5 or F9–F12 also work).
2. In the **Voice Chat** card, open *Settings*:
   - **Controller mic** is used with the controller's PS button (default: the DualSense mic). **Keyboard & mouse mic** is used with the push-to-talk key — pick your headset there. Either can be any microphone.
   - Pick an engine: **Local Whisper (offline)** — click *Download engine* and *Download model* (`small.en q5` for CPU, `large-v3-turbo q5` with *Use NVIDIA GPU*) — or a **cloud** provider (Groq, Gemini, Deepgram, ElevenLabs, Mistral, OpenAI, custom) with its API key in the keys table.
3. Add friends' names and game terms to *Names & words* so they're spelled right, and use *Auto-corrections* for anything it keeps mishearing.
4. Turn the card on. In-game, open a friend's chat (1080p layout), then tap the controller's **PS button** (tap again to finish) or hold your **push-to-talk key**:

| Gesture | Action |
|---|---|
| Tap PS, speak, tap PS (or hold the key, speak, release) | Transcribes and types the text at the end of the chat box (send it with the game's own controls) |
| Talk again | Speak another sentence; it's added after whatever is already in the chat box |
| Double-tap | Undo the last typed sentence (repeat to undo earlier ones) |

The push-to-talk key only acts while Genshin is in front and is hidden from the game. A small overlay at
the top of the screen shows listening / transcribing / the transcript. Downloads are stored in the app
data folder (`whisper/`), settings in `voice.json` in the app config folder.

## Commits & releases

Run `npm install` once at the **repository root** to enable the git hooks (husky):

- **commit-msg** — messages must follow [Conventional Commits](https://www.conventionalcommits.org/), e.g. `feat: add tray icon`, `fix: double tap not detected`, `chore: update deps`.
- **pre-commit** — checks the frontend JS parses and the Rust backend compiles.

Releases are automatic: on every push to `main`, [semantic-release](https://semantic-release.gitbook.io/) (`.github/workflows/release.yml`) reads the commits since the last release and

| Commit type | Version bump |
|---|---|
| `fix:` | patch (1.0.**1**) |
| `feat:` | minor (1.**1**.0) |
| `feat!:` or a `BREAKING CHANGE:` footer | major (**2**.0.0) |
| `chore:`, `docs:`, `ci:`, `refactor:` … | no release |

When a release is due it updates the version in `package.json`, `tauri.conf.json` and `Cargo.toml`, writes `CHANGELOG.md`, builds the Windows installers and publishes them on the GitHub Releases page.

### In-app updates

Settings → About checks the latest release's `latest.json` and installs the new version from inside the app (it also checks quietly at startup and puts a dot on the Settings tab). Installers are signed with the updater key; the app only installs files whose signature matches the public key in `tauri.conf.json`.

- Local key: `~/.tauri/genshin-utility.key`. **Back it up and never commit it** — if it's lost, installed copies can't update to new versions and everyone has to reinstall manually.
- CI: add the key file's contents as the repository secret `TAURI_SIGNING_PRIVATE_KEY`.

## Hotkeys

Every in-game hotkey can be changed in **Settings → Hotkeys**: click a key and press the new combination (Esc cancels). A hotkey only fires with exactly its modifiers, so F4 for Story Mode leaves Alt+F4 free to close the game, and two features can't share the same keys. Defaults: F4 Auto Dialogue / Story Mode, F7 / Shift+F7 artifacts (this / every character), Left / Right change characters, F8 Auto Message.
