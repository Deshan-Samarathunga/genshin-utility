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
