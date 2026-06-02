"""
Genshin Impact — Batch Artifact Remover
========================================

Fully autonomous script that unequips all artifacts from every
character in your roster with a single hotkey press.

Workflow per character:
  1. Click "Artifacts" tab in the left sidebar  →  ring view
  2. Click an equipped artifact on the ring     →  detail view
  3. Cycle through the 5 type-tabs at the top-left of the detail view
  4. For each tab: if "Remove" button is visible, click it
  5. Press Escape to return to ring view
  6. Click ">" to advance to the next character

Prerequisites:
    pip install -r requirements.txt
    python capture_templates.py          # capture reference images first

Usage:
    python batch_artifact_remover.py                 # normal run (F7 to start)
    python batch_artifact_remover.py --count 3       # process only 3 characters
    python batch_artifact_remover.py --dry-run       # log actions, don't click
    python batch_artifact_remover.py --calibrate     # show positions overlay
    python batch_artifact_remover.py --debug         # verbose logging
    python batch_artifact_remover.py --start-delay 5 # auto-start after 5s countdown

Controls:
    F7  — Start the batch run
    F9  — Stop gracefully at the next safe point
    Move mouse to any screen corner — Emergency stop (pyautogui failsafe)
"""

from __future__ import annotations

import argparse
import ctypes
import logging
import sys
import time
from pathlib import Path

import cv2
import mss
import numpy as np
import pyautogui
import yaml

# Optional: global hotkey support
try:
    import keyboard as kb

    HAS_KEYBOARD = True
except ImportError:
    HAS_KEYBOARD = False

# ── pyautogui safety ────────────────────────────────────────
pyautogui.FAILSAFE = True  # move cursor to corner → abort
pyautogui.PAUSE = 0.03  # tiny pause between pyautogui calls

# ── logging ─────────────────────────────────────────────────
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s │ %(levelname)-5s │ %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("artifact_remover")


# ═══════════════════════════════════════════════════════════
#  Config loader
# ═══════════════════════════════════════════════════════════

class Config:
    """Thin wrapper around the YAML config file."""

    def __init__(self, path: str = "config.yaml"):
        p = Path(path)
        if not p.exists():
            log.error("Config file not found: %s", p.resolve())
            sys.exit(1)
        with open(p, "r", encoding="utf-8") as fh:
            self._d = yaml.safe_load(fh)

    def __getattr__(self, key: str):
        if key.startswith("_"):
            return super().__getattribute__(key)
        try:
            return self._d[key]
        except KeyError:
            raise AttributeError(f"Config missing key: '{key}'")

    def get(self, key: str, default=None):
        return self._d.get(key, default)


# ═══════════════════════════════════════════════════════════
#  Main automation class
# ═══════════════════════════════════════════════════════════

class ArtifactRemover:
    """Iterate through all characters and remove equipped artifacts."""

    SLOT_NAMES = ["Flower", "Plume", "Sands", "Goblet", "Circlet"]

    def __init__(
        self,
        config_path: str = "config.yaml",
        dry_run: bool = False,
        count: int | None = None,
        start_delay: int | None = None,
    ):
        self.cfg = Config(config_path)
        self.dry_run = dry_run
        self.char_count = count or self.cfg.character_count
        self.start_delay = start_delay
        self.stop_flag = False
        self.sct = mss.MSS()

        # Pre-load template images
        self.templates: dict[str, np.ndarray] = {}
        self._load_templates()

        # Register stop hotkey
        if HAS_KEYBOARD:
            kb.on_press_key(self.cfg.stop_hotkey, lambda _: self._request_stop())

    # ── template loading ────────────────────────────────────

    def _load_templates(self):
        tdir = Path(self.cfg.template_dir)
        names = [
            "artifacts_tab",
            "artifacts_tab_selected",
            "remove_button",
            "slot_filled",
        ]
        for name in names:
            path = tdir / f"{name}.png"
            if path.exists():
                img = cv2.imread(str(path), cv2.IMREAD_COLOR)
                if img is not None:
                    self.templates[name] = img
                    log.info("Template loaded: %-28s (%dx%d)", name, img.shape[1], img.shape[0])
                else:
                    log.warning("Could not decode: %s", path)
            else:
                log.warning("Template missing: %s (optional)", path)

    # ── stop / focus helpers ────────────────────────────────

    def _request_stop(self):
        self.stop_flag = True
        log.info("⏹  Stop requested (%s)", self.cfg.stop_hotkey)

    def _check_stop(self) -> bool:
        return self.stop_flag

    @staticmethod
    def _is_genshin_focused() -> bool:
        """Return True if a Genshin window is in the foreground."""
        hwnd = ctypes.windll.user32.GetForegroundWindow()
        length = ctypes.windll.user32.GetWindowTextLengthW(hwnd)
        if length == 0:
            return False
        buf = ctypes.create_unicode_buffer(length + 1)
        ctypes.windll.user32.GetWindowTextW(hwnd, buf, length + 1)
        title = buf.value.lower()
        return "genshin" in title or "原神" in title

    def _wait_for_focus(self, timeout: float = 60.0):
        """Pause until the Genshin window regains focus (or timeout)."""
        if self._is_genshin_focused():
            return
        log.warning("Genshin lost focus — waiting for it to come back…")
        t0 = time.time()
        while time.time() - t0 < timeout:
            if self._check_stop():
                return
            if self._is_genshin_focused():
                log.info("Focus restored.")
                time.sleep(0.3)
                return
            time.sleep(0.5)
        log.error("Timeout waiting for Genshin focus.")
        self.stop_flag = True

    # ── screenshot helpers ──────────────────────────────────

    def _screenshot(self, region: tuple | None = None) -> np.ndarray:
        """
        Capture a screenshot.
        *region* = (x, y, width, height) or None for full primary monitor.
        Returns a BGR numpy array.
        """
        if region:
            x, y, w, h = region
            mon = {"left": x, "top": y, "width": w, "height": h}
        else:
            mon = self.sct.monitors[1]
        raw = np.array(self.sct.grab(mon))
        return cv2.cvtColor(raw, cv2.COLOR_BGRA2BGR)

    # ── template matching ───────────────────────────────────

    def _find_template(
        self,
        haystack: np.ndarray,
        needle: np.ndarray,
        threshold: float | None = None,
    ) -> tuple | None:
        """
        Run cv2.matchTemplate.
        Returns (x, y, confidence) of the best match, or None.
        """
        if threshold is None:
            threshold = self.cfg.match_threshold

        # Safety: template must fit inside the haystack
        if (needle.shape[0] > haystack.shape[0] or
                needle.shape[1] > haystack.shape[1]):
            log.debug(
                "Template (%dx%d) larger than search area (%dx%d) — skipping.",
                needle.shape[1], needle.shape[0],
                haystack.shape[1], haystack.shape[0],
            )
            return None

        result = cv2.matchTemplate(haystack, needle, cv2.TM_CCOEFF_NORMED)
        _, max_val, _, max_loc = cv2.minMaxLoc(result)
        if max_val >= threshold:
            return (max_loc[0], max_loc[1], max_val)
        return None

    def _find_on_screen(
        self, template_name: str, region: tuple, threshold: float | None = None
    ) -> tuple | None:
        """
        Find *template_name* inside a screen *region* (x1, y1, x2, y2).
        Returns absolute screen (x, y, confidence) or None.
        """
        if template_name not in self.templates:
            return None
        x1, y1, x2, y2 = region
        shot = self._screenshot((x1, y1, x2 - x1, y2 - y1))
        needle = self.templates[template_name]
        match = self._find_template(shot, needle, threshold)
        if match:
            return (match[0] + x1, match[1] + y1, match[2])
        return None

    def _wait_for_template(
        self, name: str, region: tuple, timeout_ms: int = 2000
    ) -> tuple | None:
        """Poll until *name* appears in *region*.  Returns (x, y) or None."""
        deadline = time.time() + timeout_ms / 1000
        poll = self.cfg.poll_interval_ms / 1000
        while time.time() < deadline:
            if self._check_stop():
                return None
            m = self._find_on_screen(name, region)
            if m:
                return (m[0], m[1])
            time.sleep(poll)
        return None

    def _wait_for_gone(
        self, name: str, region: tuple, timeout_ms: int = 2000
    ) -> bool:
        """Poll until *name* disappears from *region*."""
        deadline = time.time() + timeout_ms / 1000
        poll = self.cfg.poll_interval_ms / 1000
        while time.time() < deadline:
            if self._check_stop():
                return True
            if not self._find_on_screen(name, region):
                return True
            time.sleep(poll)
        return False

    # ── clicking ────────────────────────────────────────────

    def _click(self, x: int, y: int):
        """Click at (x, y).  Moves cursor back afterwards."""
        if self.dry_run:
            log.info("  [DRY] click (%d, %d)", x, y)
            return
        ox, oy = pyautogui.position()
        pyautogui.click(x, y)
        pyautogui.moveTo(ox, oy, _pause=False)

    def _sleep(self, ms: int):
        time.sleep(ms / 1000)

    # ── high-level actions ──────────────────────────────────

    def _ensure_artifacts_tab(self) -> bool:
        """Click the Artifacts tab if it is not already selected."""
        sidebar = tuple(self.cfg.sidebar_region)

        # Already selected?
        if "artifacts_tab_selected" in self.templates:
            if self._find_on_screen("artifacts_tab_selected", sidebar):
                log.debug("  Artifacts tab already active.")
                return True

        # Click it
        ax, ay = self.cfg.artifacts_tab_click
        log.debug("  Clicking Artifacts tab at (%d, %d)", ax, ay)
        self._click(ax, ay)
        self._sleep(self.cfg.after_tab_click_ms)

        # Verify
        if "artifacts_tab_selected" in self.templates:
            m = self._wait_for_template("artifacts_tab_selected", sidebar, 1500)
            if m:
                return True
            log.warning("  Could not confirm Artifacts tab selection.")
            return False

        return True  # no template → trust the click

    def _is_on_sidebar(self) -> bool:
        """Check if the left sidebar (Artifacts tab) is currently visible."""
        sidebar = tuple(self.cfg.sidebar_region)
        if "artifacts_tab_selected" in self.templates:
            return self._find_on_screen("artifacts_tab_selected", sidebar) is not None
        if "artifacts_tab" in self.templates:
            return self._find_on_screen("artifacts_tab", sidebar) is not None
        return False  # can't tell without templates

    def _enter_detail_view(self) -> bool:
        """
        From the Artifacts ring view, click an artifact to enter the
        artifact detail view.  Tries multiple ring positions.
        Returns True if the detail view was entered.
        """
        for i, pos in enumerate(self.cfg.ring_entry_clicks):
            if self._check_stop():
                return False

            log.debug("  Trying ring click #%d at (%d, %d)", i + 1, pos[0], pos[1])
            self._click(pos[0], pos[1])
            self._sleep(self.cfg.after_slot_click_ms)

            # Check if we left the ring view (sidebar should disappear)
            if not self._is_on_sidebar():
                log.debug("  Entered detail view via click #%d", i + 1)
                return True

            # Also check if the Remove button appeared (artifact was equipped)
            region = tuple(self.cfg.remove_button_search_region)
            if "remove_button" in self.templates:
                if self._find_on_screen("remove_button", region):
                    log.debug("  Entered detail view (Remove button found)")
                    return True

        log.info("  Could not enter detail view — character may have no artifacts")
        return False

    def _is_remove_visible(self) -> bool:
        """Check if the Remove button is currently visible on screen."""
        region = tuple(self.cfg.remove_button_search_region)

        if "remove_button" in self.templates:
            return self._find_on_screen("remove_button", region) is not None

        # No template: can't check, assume not visible
        return False

    def _click_remove(self) -> bool:
        """Click the Remove button.  Returns True on success."""
        region = tuple(self.cfg.remove_button_search_region)

        if "remove_button" in self.templates:
            match = self._find_on_screen("remove_button", region)
            if match:
                rx, ry = match[0], match[1]
                th, tw = self.templates["remove_button"].shape[:2]
                cx, cy = rx + tw // 2, ry + th // 2
                log.debug("    Remove found at (%d, %d) → clicking (%d, %d)", rx, ry, cx, cy)
                self._click(cx, cy)
                self._sleep(self.cfg.after_remove_click_ms)
                # Wait for the Remove button to disappear (confirms removal)
                self._wait_for_gone("remove_button", region, 2000)
                self._sleep(self.cfg.after_remove_wait_ms)
                return True
            return False
        else:
            # Fallback: click at fixed coordinates
            rx, ry = self.cfg.remove_button_pos
            log.debug("    Remove at fixed pos (%d, %d)", rx, ry)
            self._click(rx, ry)
            self._sleep(self.cfg.after_remove_click_ms + self.cfg.after_remove_wait_ms)
            return True

    def _remove_artifacts_for_character(self) -> int:
        """
        Remove all equipped artifacts from the current character.

        Strategy:
          1. Enter the artifact detail view by clicking a slot on the ring.
          2. Cycle through the 5 type-tabs at the top of the detail view.
          3. For each tab, check if the Remove button is visible and click it.
          4. Press Escape to return to the ring view.
        """
        # Step 1: enter the detail view
        if not self._enter_detail_view():
            return 0

        removed = 0

        # Step 2: cycle through all 5 artifact type tabs
        for idx, tab_pos in enumerate(self.cfg.artifact_type_tabs):
            if self._check_stop():
                break

            name = self.SLOT_NAMES[idx] if idx < len(self.SLOT_NAMES) else f"Slot{idx+1}"

            # Click the type tab
            log.debug("    Clicking %s tab at (%d, %d)", name, tab_pos[0], tab_pos[1])
            self._click(tab_pos[0], tab_pos[1])
            self._sleep(self.cfg.after_type_tab_switch_ms)

            # Check if Remove button is visible → artifact is equipped
            if self._is_remove_visible():
                log.info("    %s — equipped → removing…", name)
                if self._click_remove():
                    removed += 1
                    log.info("    %s — removed ✓", name)
                else:
                    log.warning("    %s — removal failed ✗", name)
            else:
                log.debug("    %s — not equipped, skipping", name)

        # Step 3: exit back to the ring view
        log.debug("  Pressing Escape to return to ring view")
        if not self.dry_run:
            pyautogui.press("escape")
        self._sleep(500)

        return removed

    def _next_character(self):
        """Click the right arrow to advance to the next character."""
        rx, ry = self.cfg.character_right_arrow
        log.debug("  Next character → (%d, %d)", rx, ry)
        self._click(rx, ry)
        self._sleep(self.cfg.after_character_switch_ms)

    # ── calibration mode ────────────────────────────────────

    def calibrate(self):
        """
        Show a screenshot with all configured positions annotated,
        so the user can verify the coordinates match their game UI.
        """
        log.info("Taking calibration screenshot…")
        time.sleep(1)
        img = self._screenshot()

        # ── Ring entry clicks (green circles) ──
        for i, pos in enumerate(self.cfg.ring_entry_clicks):
            x, y = pos
            cv2.circle(img, (x, y), 18, (0, 255, 0), 2)
            cv2.putText(img, f"Ring{i+1}", (x + 22, y + 6),
                        cv2.FONT_HERSHEY_SIMPLEX, 0.5, (0, 255, 0), 2)

        # ── Artifact type tabs (cyan boxes) ──
        for i, pos in enumerate(self.cfg.artifact_type_tabs):
            x, y = pos
            label = self.SLOT_NAMES[i] if i < len(self.SLOT_NAMES) else f"T{i+1}"
            cv2.rectangle(img, (x - 20, y - 20), (x + 20, y + 20), (255, 255, 0), 2)
            cv2.putText(img, label, (x - 20, y + 35),
                        cv2.FONT_HERSHEY_SIMPLEX, 0.45, (255, 255, 0), 1)

        # ── Artifacts tab click (orange) ──
        ax, ay = self.cfg.artifacts_tab_click
        cv2.circle(img, (ax, ay), 12, (255, 100, 0), 3)
        cv2.putText(img, "Artifacts Tab", (ax + 18, ay + 5),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.55, (255, 100, 0), 2)

        # ── Character right arrow (red) ──
        rx, ry = self.cfg.character_right_arrow
        cv2.circle(img, (rx, ry), 12, (0, 0, 255), 3)
        cv2.putText(img, "Next Char >", (rx - 120, ry - 18),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.55, (0, 0, 255), 2)

        # ── Remove button (yellow) ──
        bx, by = self.cfg.remove_button_pos
        cv2.circle(img, (bx, by), 12, (0, 180, 255), 3)
        cv2.putText(img, "Remove Btn", (bx + 18, by + 5),
                    cv2.FONT_HERSHEY_SIMPLEX, 0.55, (0, 180, 255), 2)

        # ── Remove search region (dashed yellow) ──
        r = self.cfg.remove_button_search_region
        cv2.rectangle(img, (r[0], r[1]), (r[2], r[3]), (0, 180, 255), 1)

        # ── Sidebar region (orange outline) ──
        s = self.cfg.sidebar_region
        cv2.rectangle(img, (s[0], s[1]), (s[2], s[3]), (255, 100, 0), 1)

        # Show
        cv2.namedWindow("Calibration", cv2.WINDOW_NORMAL)
        cv2.resizeWindow("Calibration", 1280, 720)
        cv2.imshow("Calibration", img)
        log.info("Calibration overlay shown.  Press any key to close.")
        log.info("  Green circles  = ring entry clicks (to enter detail view)")
        log.info("  Cyan boxes     = artifact type tabs (top of detail view)")
        log.info("  Orange         = Artifacts tab & sidebar region")
        log.info("  Red            = Next character arrow")
        log.info("  Yellow         = Remove button & search region")
        cv2.waitKey(0)
        cv2.destroyAllWindows()

    # ── main loop ───────────────────────────────────────────

    def run(self):
        """Entry point: wait for hotkey, then process all characters."""
        hdr = "═" * 62
        log.info(hdr)
        log.info("  Genshin Impact — Batch Artifact Remover")
        log.info("  Characters : %d", self.char_count)
        log.info("  Dry run    : %s", self.dry_run)
        log.info("  Templates  : %d loaded", len(self.templates))
        log.info(hdr)

        # ── wait for trigger ──
        if self.start_delay is not None:
            log.info("Auto-start in %d seconds…  (F9 to cancel)", self.start_delay)
            for remaining in range(self.start_delay, 0, -1):
                if self._check_stop():
                    log.info("Cancelled.")
                    return
                log.info("  %d…", remaining)
                time.sleep(1)
        elif HAS_KEYBOARD:
            log.info("Press %s to START  |  %s to STOP at any time",
                     self.cfg.start_hotkey, self.cfg.stop_hotkey)
            kb.wait(self.cfg.start_hotkey)
            if self._check_stop():
                log.info("Cancelled before start.")
                return
        else:
            input("Press Enter to start…")

        # ── preflight ──
        self._wait_for_focus()
        if self._check_stop():
            return

        log.info("▶  Starting batch artifact removal…")

        total_removed = 0
        processed = 0
        t0 = time.time()

        for ci in range(1, self.char_count + 1):
            if self._check_stop():
                break

            # Focus guard
            self._wait_for_focus()
            if self._check_stop():
                break

            log.info("━━  Character %d / %d  ━━━━━━━━━━━━━━━━━━━━━━━━", ci, self.char_count)

            # Navigate to Artifacts tab
            if not self._ensure_artifacts_tab():
                log.error("Cannot reach Artifacts tab — aborting.")
                break

            # Remove all artifacts for this character
            removed = self._remove_artifacts_for_character()
            total_removed += removed
            processed += 1
            log.info("  Character %d done — removed %d artifact(s)  (running total: %d)",
                     ci, removed, total_removed)

            # Advance to next character (skip for the last one)
            if ci < self.char_count:
                self._next_character()

        elapsed = time.time() - t0
        log.info(hdr)
        log.info("  FINISHED")
        log.info("  Characters processed : %d", processed)
        log.info("  Artifacts removed    : %d", total_removed)
        log.info("  Time elapsed         : %.0fs  (%.1f min)", elapsed, elapsed / 60)
        log.info(hdr)


# ═══════════════════════════════════════════════════════════
#  CLI
# ═══════════════════════════════════════════════════════════

def main():
    ap = argparse.ArgumentParser(
        description="Genshin Impact — Batch Artifact Remover",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="Press F7 to start, F9 to stop.  Move mouse to corner = emergency stop.",
    )
    ap.add_argument("--config", default="config.yaml", help="Path to config YAML")
    ap.add_argument("--count", type=int, help="Process only N characters (overrides config)")
    ap.add_argument("--dry-run", action="store_true", help="Log all actions without clicking")
    ap.add_argument("--calibrate", action="store_true", help="Show positions overlay on screenshot")
    ap.add_argument("--debug", action="store_true", help="Enable verbose debug logging")
    ap.add_argument(
        "--start-delay", type=int, metavar="SEC",
        help="Auto-start after SEC-second countdown instead of waiting for hotkey",
    )

    args = ap.parse_args()

    if args.debug:
        logging.getLogger().setLevel(logging.DEBUG)

    remover = ArtifactRemover(
        config_path=args.config,
        dry_run=args.dry_run,
        count=args.count,
        start_delay=args.start_delay,
    )

    if args.calibrate:
        remover.calibrate()
    else:
        remover.run()


if __name__ == "__main__":
    main()
