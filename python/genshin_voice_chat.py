"""
Genshin Impact — Keyboard-less Voice Chat (PS5 DualSense)
=========================================================

A self-contained replacement for the AutoHotkey approach. Instead of
fighting DirectInput + Win+H, this talks to the controller directly and
runs speech-to-text locally:

    DualSense (HID)  ──►  pydualsense        read buttons + drive LED/rumble
    Controller mic   ──►  sounddevice        capture your voice
    Audio            ──►  faster-whisper     offline, accurate dictation
    Text + keys      ──►  Win32 SendInput    typed into Genshin's chat box

Why this beats AHK for this job:
  * Voice is transcribed by Whisper *offline* — far better than Win+H, and
    it can listen through the DualSense's own built-in microphone.
  * Buttons are read by name ("touchpad", "options", "l3"…), not by fragile
    Joy-numbers that change per driver.
  * The controller LED and haptics give you feedback: green = idle,
    blue = listening, orange = transcribing, a buzz when a message sends.

In-game workflow:
  1. Open Genshin's co-op chat box so the text field has focus.
  2. Press VOICE → speak → press VOICE again to stop (LED shows the state).
  3. Fix mistakes with SPACE / BACKSPACE / CLEAR.
  4. Press SUBMIT to send.

Honest limitations (please read once):
  * Like any user-space tool, reading the pad does NOT stop the press from
    also reaching Genshin. Map *chat-safe* buttons (touchpad, options,
    create/share, stick-clicks, mic-mute) — not movement/attack buttons.
  * If you route the pad through Steam Input or DS4Windows, the real HID
    device is hidden and pydualsense can't see it. Use Genshin's *native*
    PlayStation controller support instead (Settings → Controller), or
    connect a second pad for gameplay.
  * Text injection uses the WM_CHAR/Unicode path (the same one IMEs use),
    which Genshin's chat box accepts. Special keys use hardware scancodes.

Prerequisites:
    pip install -r requirements-voice.txt
    # first run downloads the Whisper model (a few hundred MB for base.en)

Usage:
    python genshin_voice_chat.py                       # run with config defaults
    python genshin_voice_chat.py --list-audio          # list input devices, then exit
    python genshin_voice_chat.py --model small.en      # better accuracy, slower
    python genshin_voice_chat.py --voice-mode push_to_talk
    python genshin_voice_chat.py --debug

Controls (all remappable in voice_chat_config.yaml):
    Touchpad click   — start/stop voice dictation
    L3 (left stick)  — insert a space
    R3 (right stick) — delete last character (hold to repeat)
    Create / Share   — clear the whole chat box
    Options          — send the message (Enter)
    Mic-mute (hold)  — quit this script   (Ctrl+C in the console also works)
"""

from __future__ import annotations

import argparse
import ctypes
import logging
import threading
import time
from ctypes import wintypes
from pathlib import Path

# YAML is optional — without it we fall back to the built-in defaults.
try:
    import yaml
except ImportError:
    yaml = None

# Audio stack. Imported defensively so --help / --list-audio still work and
# so a missing PortAudio backend produces a clear message instead of a crash.
try:
    import numpy as np
    import sounddevice as sd
except Exception:  # ImportError, or OSError if PortAudio is absent
    np = None
    sd = None


# ── logging (matches the style of batch_artifact_remover.py) ─────────
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s │ %(levelname)-5s │ %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("voice_chat")


# ═══════════════════════════════════════════════════════════
#  Config (YAML with built-in defaults)
# ═══════════════════════════════════════════════════════════

DEFAULTS: dict = {
    # "toggle" = press once to start, again to stop.
    # "push_to_talk" = hold to dictate, release to stop.
    "voice_mode": "toggle",

    # Logical action  ->  DualSense button name. See voice_chat_config.yaml
    # for the full list of valid names. These are the chat-safe defaults.
    "buttons": {
        "voice": "touchpad",   # touchpad click
        "space": "l3",         # left stick click
        "backspace": "r3",     # right stick click
        "clear": "share",      # Create / Share button
        "submit": "options",   # Options button
        "quit": "mic",         # mic-mute button (hold ~1.2s to exit)
    },

    # Speech-to-text (faster-whisper). Models: tiny.en, base.en, small.en,
    # medium.en … larger = more accurate but slower. For GPU set device
    # "cuda" and compute_type "float16".
    "whisper_model": "base.en",
    "whisper_device": "cpu",
    "whisper_compute_type": "int8",
    "language": "en",

    # Substring used to pick the input device. The DualSense mic usually
    # shows up as "Wireless Controller". Set to "" to use the Windows
    # default microphone instead.
    "mic_name_contains": "Wireless Controller",

    # Only act while a Genshin window is in the foreground (the equivalent
    # of AHK's #IfWinActive). The window title is matched case-insensitively.
    "only_when_genshin_focused": True,
    "genshin_title_contains": ["genshin", "原神"],

    # Clearing the chat box.
    "clear_use_select_all": True,   # Ctrl+A then Backspace (fast + clean)
    "clear_backspaces": 60,         # used only when the above is False

    # Hold BACKSPACE to delete many characters.
    "backspace_repeat_delay": 0.35,     # seconds held before auto-repeat
    "backspace_repeat_interval": 0.06,  # seconds between repeats

    "poll_hz": 120,            # controller poll rate
    "haptics": True,           # rumble feedback on actions
    "led_feedback": True,      # LED colour reflects state
    "type_char_delay": 0.0,    # delay between injected characters (0 = fastest)
    "stop_voice_on_submit": True,
}


class Config:
    """Built-in defaults, optionally overridden by a YAML file."""

    def __init__(self, path: str | None = None):
        d = dict(DEFAULTS)
        d["buttons"] = dict(DEFAULTS["buttons"])
        d["genshin_title_contains"] = list(DEFAULTS["genshin_title_contains"])

        if path:
            p = Path(path)
            if p.exists():
                if yaml is None:
                    log.warning("PyYAML not installed — ignoring %s, using defaults.", p)
                else:
                    with open(p, "r", encoding="utf-8") as fh:
                        loaded = yaml.safe_load(fh) or {}
                    btns = loaded.pop("buttons", None)
                    d.update(loaded)
                    if btns:
                        d["buttons"].update(btns)
                    log.info("Loaded config: %s", p)
            else:
                log.info("No config file at %s — using built-in defaults.", p)
        self._d = d

    def __getattr__(self, key: str):
        if key.startswith("_"):
            raise AttributeError(key)
        try:
            return self._d[key]
        except KeyError:
            raise AttributeError(f"Config missing key: '{key}'")


# ═══════════════════════════════════════════════════════════
#  Win32 SendInput — text + key injection
# ═══════════════════════════════════════════════════════════

_user32 = ctypes.WinDLL("user32", use_last_error=True)
_ULONG_PTR = ctypes.c_uint64 if ctypes.sizeof(ctypes.c_void_p) == 8 else ctypes.c_uint32

_INPUT_KEYBOARD = 1
_KEYEVENTF_EXTENDEDKEY = 0x0001
_KEYEVENTF_KEYUP = 0x0002
_KEYEVENTF_UNICODE = 0x0004
_KEYEVENTF_SCANCODE = 0x0008


class _KEYBDINPUT(ctypes.Structure):
    _fields_ = (
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", _ULONG_PTR),
    )


class _MOUSEINPUT(ctypes.Structure):
    _fields_ = (
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", _ULONG_PTR),
    )


class _INPUTUNION(ctypes.Union):
    _fields_ = (("ki", _KEYBDINPUT), ("mi", _MOUSEINPUT))


class _INPUT(ctypes.Structure):
    _fields_ = (("type", wintypes.DWORD), ("u", _INPUTUNION))


_user32.SendInput.argtypes = (wintypes.UINT, ctypes.POINTER(_INPUT), ctypes.c_int)
_user32.SendInput.restype = wintypes.UINT

# Hardware scancodes (set 1) for the special keys we need.
_SCAN = {"backspace": 0x0E, "enter": 0x1C, "space": 0x39, "ctrl": 0x1D, "a": 0x1E}


def _unicode_input(code_unit: int, keyup: bool) -> _INPUT:
    flags = _KEYEVENTF_UNICODE | (_KEYEVENTF_KEYUP if keyup else 0)
    ki = _KEYBDINPUT(0, code_unit, flags, 0, 0)
    return _INPUT(_INPUT_KEYBOARD, _INPUTUNION(ki=ki))


def _scan_input(scan: int, keyup: bool, extended: bool = False) -> _INPUT:
    flags = _KEYEVENTF_SCANCODE
    if extended:
        flags |= _KEYEVENTF_EXTENDEDKEY
    if keyup:
        flags |= _KEYEVENTF_KEYUP
    ki = _KEYBDINPUT(0, scan, flags, 0, 0)
    return _INPUT(_INPUT_KEYBOARD, _INPUTUNION(ki=ki))


def _send(events: list) -> None:
    n = len(events)
    arr = (_INPUT * n)(*events)
    _user32.SendInput(n, arr, ctypes.sizeof(_INPUT))


class KeyInjector:
    """Types arbitrary text and presses control keys into the focused window."""

    def __init__(self, char_delay: float = 0.0):
        self.char_delay = char_delay

    def type_text(self, text: str) -> None:
        """Send `text` as Unicode (WM_CHAR). Handles emoji / surrogate pairs."""
        units = text.encode("utf-16-le")
        for i in range(0, len(units), 2):
            cu = units[i] | (units[i + 1] << 8)
            _send([_unicode_input(cu, False), _unicode_input(cu, True)])
            if self.char_delay:
                time.sleep(self.char_delay)

    def tap(self, name: str, repeat: int = 1) -> None:
        """Tap a named special key (backspace/enter/space) via scancode."""
        scan = _SCAN[name]
        for _ in range(repeat):
            _send([_scan_input(scan, False), _scan_input(scan, True)])

    def select_all_then_delete(self) -> None:
        """Ctrl+A then Backspace — clears the whole field in one shot."""
        _send([
            _scan_input(_SCAN["ctrl"], False),
            _scan_input(_SCAN["a"], False),
            _scan_input(_SCAN["a"], True),
            _scan_input(_SCAN["ctrl"], True),
        ])
        time.sleep(0.02)
        self.tap("backspace")


# ═══════════════════════════════════════════════════════════
#  Window focus gate (the #IfWinActive equivalent)
# ═══════════════════════════════════════════════════════════

def is_genshin_focused(title_substrings) -> bool:
    hwnd = ctypes.windll.user32.GetForegroundWindow()
    if not hwnd:
        return False
    length = ctypes.windll.user32.GetWindowTextLengthW(hwnd)
    if length == 0:
        return False
    buf = ctypes.create_unicode_buffer(length + 1)
    ctypes.windll.user32.GetWindowTextW(hwnd, buf, length + 1)
    title = buf.value.lower()
    return any(str(s).lower() in title for s in title_substrings)


# ═══════════════════════════════════════════════════════════
#  DualSense controller (input + LED/haptic feedback)
# ═══════════════════════════════════════════════════════════

# Friendly config name  ->  pydualsense DSState attribute.
_ATTR_BY_NAME = {
    "cross": "cross", "x": "cross",
    "circle": "circle", "square": "square", "triangle": "triangle",
    "l1": "L1", "r1": "R1", "l3": "L3", "r3": "R3",
    "dpad_up": "DpadUp", "dpad_down": "DpadDown",
    "dpad_left": "DpadLeft", "dpad_right": "DpadRight",
    "share": "share", "create": "share",
    "options": "options", "ps": "ps",
    "touchpad": "touchBtn", "touchpad_click": "touchBtn",
    "mic": "micBtn", "mute": "micBtn",
}

VALID_BUTTON_NAMES = sorted(set(_ATTR_BY_NAME) | {"l2", "r2"})


class DualSense:
    """Thin wrapper over pydualsense: poll buttons, set LED, pulse rumble."""

    STATUS_COLORS = {
        "idle": (0, 60, 0),          # dim green
        "listening": (0, 0, 255),    # blue
        "transcribing": (255, 140, 0),  # orange
        "loading": (120, 0, 200),    # purple
        "off": (0, 0, 0),
    }

    def __init__(self, cfg: Config):
        try:
            from pydualsense import pydualsense
        except Exception as e:
            raise RuntimeError("pydualsense not installed — pip install pydualsense") from e
        self.cfg = cfg
        self.ds = pydualsense()
        self.ds.init()  # raises if no controller is connected
        self.set_status("loading")

    def read(self):
        return self.ds.state

    def is_pressed(self, state, name: str) -> bool:
        name = (name or "").strip().lower()
        if name in ("l2", "r2"):
            analog = getattr(state, "L2" if name == "l2" else "R2", 0) or 0
            btn = getattr(state, "L2Btn" if name == "l2" else "R2Btn", None)
            return bool(btn) if btn is not None else (analog > 200)
        attr = _ATTR_BY_NAME.get(name)
        if not attr:
            return False
        return bool(getattr(state, attr, False))

    def set_status(self, status: str) -> None:
        if not self.cfg.led_feedback:
            return
        rgb = self.STATUS_COLORS.get(status, self.STATUS_COLORS["idle"])
        try:
            self.ds.light.setColorI(*rgb)
        except Exception:
            pass  # feedback is cosmetic — never let it break the app

    def pulse(self, intensity: int = 110, ms: int = 70) -> None:
        if not self.cfg.haptics:
            return

        def _buzz():
            try:
                self.ds.setLeftMotor(intensity)
                self.ds.setRightMotor(intensity)
                time.sleep(ms / 1000.0)
                self.ds.setLeftMotor(0)
                self.ds.setRightMotor(0)
            except Exception:
                pass

        threading.Thread(target=_buzz, daemon=True).start()

    def close(self) -> None:
        for fn in (
            lambda: self.ds.light.setColorI(0, 0, 0),
            lambda: self.ds.setLeftMotor(0),
            lambda: self.ds.setRightMotor(0),
            lambda: self.ds.close(),
        ):
            try:
                fn()
            except Exception:
                pass


# ═══════════════════════════════════════════════════════════
#  Voice capture + transcription
# ═══════════════════════════════════════════════════════════

class VoiceTyper:
    """Records from the chosen mic and transcribes with faster-whisper."""

    TARGET_RATE = 16000  # Whisper expects 16 kHz mono float32

    def __init__(self, cfg: Config):
        self.cfg = cfg
        self.device = self._find_device(cfg.mic_name_contains)
        self._frames: list = []
        self._stream = None
        self._cap_rate = self.TARGET_RATE
        self._model = None

    # ── device selection ──
    def _find_device(self, name_contains: str):
        if not name_contains:
            return None
        try:
            for idx, dev in enumerate(sd.query_devices()):
                if dev.get("max_input_channels", 0) > 0 and \
                        name_contains.lower() in dev["name"].lower():
                    log.info("Mic: [%d] %s", idx, dev["name"])
                    return idx
        except Exception as e:
            log.warning("Audio device query failed: %s", e)
        log.info("Mic '%s' not found — using the system default input.", name_contains)
        return None

    def _choose_rate(self) -> int:
        """16 kHz if the device supports it, else its native rate (resampled later)."""
        try:
            sd.check_input_settings(device=self.device, samplerate=self.TARGET_RATE,
                                    channels=1, dtype="float32")
            return self.TARGET_RATE
        except Exception:
            try:
                info = sd.query_devices(self.device, "input")
                return int(info["default_samplerate"])
            except Exception:
                return 48000

    # ── model ──
    def load_model(self) -> None:
        if self._model is not None:
            return
        from faster_whisper import WhisperModel
        log.info("Loading Whisper '%s' (%s / %s)...",
                 self.cfg.whisper_model, self.cfg.whisper_device, self.cfg.whisper_compute_type)
        self._model = WhisperModel(
            self.cfg.whisper_model,
            device=self.cfg.whisper_device,
            compute_type=self.cfg.whisper_compute_type,
        )
        log.info("Whisper ready.")

    # ── recording ──
    def start(self) -> None:
        self._frames = []
        self._cap_rate = self._choose_rate()
        self._stream = sd.InputStream(
            samplerate=self._cap_rate, channels=1, dtype="float32",
            device=self.device, callback=self._callback,
        )
        self._stream.start()

    def _callback(self, indata, frames, time_info, status):
        self._frames.append(indata.copy())

    def stop(self):
        if self._stream is None:
            return np.zeros(0, dtype=np.float32)
        try:
            self._stream.stop()
            self._stream.close()
        finally:
            self._stream = None
        if not self._frames:
            return np.zeros(0, dtype=np.float32)
        audio = np.concatenate(self._frames, axis=0).reshape(-1)
        return self._resample(audio, self._cap_rate)

    def _resample(self, audio, src_rate: int):
        if src_rate == self.TARGET_RATE or audio.size == 0:
            return audio
        n_out = int(audio.shape[0] * self.TARGET_RATE / src_rate)
        if n_out <= 0:
            return np.zeros(0, dtype=np.float32)
        x_old = np.linspace(0.0, 1.0, num=audio.shape[0], endpoint=False)
        x_new = np.linspace(0.0, 1.0, num=n_out, endpoint=False)
        return np.interp(x_new, x_old, audio).astype(np.float32)

    # ── transcription ──
    def transcribe(self, audio) -> str:
        if audio is None or audio.size == 0:
            return ""
        self.load_model()
        segments, _ = self._model.transcribe(
            audio, language=self.cfg.language, vad_filter=True, beam_size=1,
        )
        return "".join(seg.text for seg in segments).strip()


# ═══════════════════════════════════════════════════════════
#  Application — edge-detected polling loop + action dispatch
# ═══════════════════════════════════════════════════════════

class VoiceChatApp:
    ACTIONS = ("voice", "space", "backspace", "clear", "submit", "quit")

    def __init__(self, cfg: Config):
        self.cfg = cfg
        self.injector = KeyInjector(char_delay=cfg.type_char_delay)
        self.voice = VoiceTyper(cfg)
        self.pad: DualSense | None = None
        self.listening = False
        self.busy = False
        self.stop = False
        self.prev = {a: False for a in self.ACTIONS}
        self.held_since = {a: 0.0 for a in self.ACTIONS}
        self.last_repeat = {a: 0.0 for a in self.ACTIONS}

    def _button_for(self, action: str) -> str:
        return self.cfg.buttons.get(action, "")

    # ── lifecycle ──
    def run(self) -> None:
        try:
            self.pad = DualSense(self.cfg)
        except Exception as e:
            log.error("Could not open the DualSense: %s", e)
            log.error("Tips: connect via USB or pair Bluetooth, and disable "
                      "Steam Input / DS4Windows so the HID device is visible.")
            return

        self._validate_buttons()

        # Preload Whisper so the first dictation is instant, not a 5s stall.
        self.pad.set_status("loading")
        try:
            self.voice.load_model()
        except Exception as e:
            log.error("Whisper failed to load: %s", e)
            self.pad.close()
            return
        self.pad.set_status("idle")

        self._banner()
        period = 1.0 / max(30, self.cfg.poll_hz)
        try:
            while not self.stop:
                gated = (not self.cfg.only_when_genshin_focused) \
                    or is_genshin_focused(self.cfg.genshin_title_contains)
                state = self.pad.read()
                now = time.monotonic()
                for a in self.ACTIONS:
                    raw = self.pad.is_pressed(state, self._button_for(a))
                    # 'quit' must work even when Genshin is not in focus.
                    pressed = raw if a == "quit" else (gated and raw)
                    was = self.prev[a]
                    if pressed and not was:
                        self.held_since[a] = now
                        self.last_repeat[a] = now
                        self._on_press(a)
                    elif was and not pressed:
                        self._on_release(a)
                    elif pressed and was:
                        self._on_hold(a, now)
                    self.prev[a] = pressed
                time.sleep(period)
        except KeyboardInterrupt:
            pass
        finally:
            self._shutdown()

    def _validate_buttons(self) -> None:
        for a in self.ACTIONS:
            b = self._button_for(a).lower()
            if b and b not in VALID_BUTTON_NAMES:
                log.warning("Action '%s' → unknown button '%s' (does nothing). "
                            "Valid: %s", a, b, ", ".join(VALID_BUTTON_NAMES))

    # ── input edges ──
    def _on_press(self, a: str) -> None:
        if a == "voice":
            if self.cfg.voice_mode == "push_to_talk":
                self._start_listen()
            else:
                self._toggle_listen()
        elif a == "space":
            self.injector.tap("space")
            self.pad.pulse(60, 40)
        elif a == "backspace":
            self.injector.tap("backspace")
        elif a == "clear":
            self._clear()
        elif a == "submit":
            self._submit()
        # 'quit' is handled on hold

    def _on_release(self, a: str) -> None:
        if a == "voice" and self.cfg.voice_mode == "push_to_talk":
            self._stop_and_type()

    def _on_hold(self, a: str, now: float) -> None:
        if a == "backspace" and self.cfg.backspace_repeat_delay > 0:
            if (now - self.held_since[a] >= self.cfg.backspace_repeat_delay and
                    now - self.last_repeat[a] >= self.cfg.backspace_repeat_interval):
                self.last_repeat[a] = now
                self.injector.tap("backspace")
        elif a == "quit":
            if now - self.held_since[a] >= 1.2:
                log.info("Quit button held — exiting.")
                self.stop = True

    # ── voice ──
    def _toggle_listen(self) -> None:
        if self.listening:
            self._stop_and_type()
        else:
            self._start_listen()

    def _start_listen(self) -> None:
        if self.listening or self.busy:
            return
        try:
            self.voice.start()
        except Exception as e:
            log.error("Mic start failed: %s", e)
            return
        self.listening = True
        self.pad.set_status("listening")
        log.info("Listening...")

    def _stop_and_type(self) -> None:
        if not self.listening:
            return
        self.listening = False
        audio = self.voice.stop()
        self.busy = True
        self.pad.set_status("transcribing")
        log.info("Transcribing %.1fs of audio...", audio.size / VoiceTyper.TARGET_RATE)
        threading.Thread(target=self._transcribe_worker, args=(audio,), daemon=True).start()

    def _transcribe_worker(self, audio) -> None:
        try:
            text = self.voice.transcribe(audio)
            if text:
                log.info('Text: "%s"', text)
                self.injector.type_text(text)
                self.pad.pulse()
            else:
                log.info("(no speech detected)")
        except Exception as e:
            log.error("Transcription error: %s", e)
        finally:
            self.busy = False
            self.pad.set_status("idle")

    # ── editing ──
    def _clear(self) -> None:
        if self.cfg.clear_use_select_all:
            self.injector.select_all_then_delete()
        else:
            self.injector.tap("backspace", repeat=self.cfg.clear_backspaces)
        self.pad.pulse()

    def _submit(self) -> None:
        # Finish any in-progress dictation first so the text lands before Enter.
        if self.listening:
            self._stop_and_type()
        if self.cfg.stop_voice_on_submit:
            deadline = time.monotonic() + 4.0
            while self.busy and time.monotonic() < deadline:
                time.sleep(0.03)
        self.injector.tap("enter")
        self.pad.pulse(140, 90)
        log.info("Sent.")

    # ── misc ──
    def _banner(self) -> None:
        bar = "═" * 60
        log.info(bar)
        log.info("  Genshin Voice Chat - DualSense  (mode: %s)", self.cfg.voice_mode)
        for a in self.ACTIONS:
            log.info("    %-10s -> %s", a, self._button_for(a))
        log.info("  LED: green=idle  blue=listening  orange=transcribing")
        log.info("  Hold the quit button (or Ctrl+C) to exit.")
        log.info(bar)

    def _shutdown(self) -> None:
        if self.pad:
            self.pad.close()
        log.info("Stopped.")


# ═══════════════════════════════════════════════════════════
#  CLI
# ═══════════════════════════════════════════════════════════

def _list_audio() -> None:
    if sd is None:
        print("sounddevice not available — pip install -r requirements-voice.txt")
        return
    print("Input devices:")
    for idx, dev in enumerate(sd.query_devices()):
        if dev.get("max_input_channels", 0) > 0:
            print(f"  [{idx}] {dev['name']}  (in:{dev['max_input_channels']})")


def main() -> None:
    ap = argparse.ArgumentParser(
        description="Genshin Impact — keyboard-less voice chat for the PS5 DualSense",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("--config", default="voice_chat_config.yaml", help="Path to YAML config")
    ap.add_argument("--model", help="Override Whisper model (tiny.en, base.en, small.en…)")
    ap.add_argument("--voice-mode", choices=["toggle", "push_to_talk"],
                    help="Override the voice activation mode")
    ap.add_argument("--list-audio", action="store_true", help="List input devices and exit")
    ap.add_argument("--debug", action="store_true", help="Verbose logging")
    args = ap.parse_args()

    if args.debug:
        logging.getLogger().setLevel(logging.DEBUG)

    if args.list_audio:
        _list_audio()
        return

    if sd is None or np is None:
        log.error("Audio stack unavailable. Install dependencies:")
        log.error("    pip install -r requirements-voice.txt")
        return

    cfg = Config(args.config)
    if args.model:
        cfg._d["whisper_model"] = args.model
    if args.voice_mode:
        cfg._d["voice_mode"] = args.voice_mode

    VoiceChatApp(cfg).run()


if __name__ == "__main__":
    main()
