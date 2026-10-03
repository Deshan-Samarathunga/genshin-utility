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

## Voice Chat (PS5)

Type to friends in Genshin's chat from the couch using the DualSense's built-in microphone.

1. Connect the DualSense **by USB** (Windows only exposes the controller mic over USB) and use Genshin's native controller support (not Steam Input / DS4Windows).
2. In the **Voice Chat (PS5)** card, open *Settings* and pick an engine:
   - **Local Whisper (offline):** click *Download engine* (whisper.cpp `whisper-server`, CPU or CUDA build) and *Download model*. `small.en q5` is a good CPU default; with *Use NVIDIA GPU* on, `large-v3-turbo q5` is the most accurate.
   - **Cloud Whisper:** any OpenAI-compatible endpoint, e.g. Groq (`https://api.groq.com/openai/v1`, model `whisper-large-v3`) with your API key.
3. Add friends' names and game terms to *Names & words* so they're spelled right, and use *Auto-corrections* for anything it keeps mishearing.
4. Turn the card on. In-game, open a friend's chat (1080p layout), then on the controller:

| Gesture on the **mic button** | Action |
|---|---|
| Hold, speak, release | Transcribes and types the text into the chat box (not sent yet) |
| Tap | Send |
| Double-tap | Discard the typed text |
| Hold again | Discard and record a new take |

A small overlay at the top of the screen shows listening / transcribing / the transcript. Downloads are stored in the app data folder (`whisper/`), settings in `voice.json` in the app config folder.
