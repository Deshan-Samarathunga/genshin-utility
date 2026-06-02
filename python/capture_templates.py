"""
Template Image Capture Utility for Genshin Batch Artifact Remover
=================================================================

Interactive tool to capture reference images from your game screen.
Run this while Genshin Impact is open and on the relevant screen.

Usage:
    python capture_templates.py

For each template you will:
  1. Navigate to the correct game screen (instructions are shown).
  2. Press SPACE to take a screenshot.
  3. Draw a rectangle around the target UI element.
  4. Press ENTER to save, or ESC to retry.
"""

import sys
import time
from pathlib import Path

import cv2
import numpy as np
import mss

try:
    import keyboard
    HAS_KEYBOARD = True
except ImportError:
    HAS_KEYBOARD = False

TEMPLATE_DIR = Path("assets/templates")
TEMPLATE_DIR.mkdir(parents=True, exist_ok=True)


# ── Interactive rectangle selector ─────────────────────────

class RectangleSelector:
    """Let the user drag-select a rectangle on an image."""

    def __init__(self, image: np.ndarray, title: str = "Select Region"):
        self.original = image.copy()
        self.display = image.copy()
        self.title = title
        self.start = None
        self.end = None
        self.dragging = False

    # ── mouse callback ──

    def _on_mouse(self, event, x, y, flags, _):
        if event == cv2.EVENT_LBUTTONDOWN:
            self.start = (x, y)
            self.dragging = True
        elif event == cv2.EVENT_MOUSEMOVE and self.dragging:
            self.display = self.original.copy()
            cv2.rectangle(self.display, self.start, (x, y), (0, 255, 0), 2)
        elif event == cv2.EVENT_LBUTTONUP:
            self.end = (x, y)
            self.dragging = False
            cv2.rectangle(self.display, self.start, self.end, (0, 255, 0), 2)

    # ── run the selector ──

    def run(self) -> tuple | None:
        """
        Show the image and let the user draw a rectangle.
        Returns (x, y, w, h) in *original-image* coordinates, or None on cancel.
        """
        h, w = self.original.shape[:2]
        scale = min(1.0, 1600 / w, 900 / h)

        if scale < 1.0:
            dw, dh = int(w * scale), int(h * scale)
            self.display = cv2.resize(self.original, (dw, dh))
            self.original = self.display.copy()
        else:
            scale = 1.0

        cv2.namedWindow(self.title, cv2.WINDOW_AUTOSIZE)
        cv2.setMouseCallback(self.title, self._on_mouse)

        print("    → Draw a rectangle around the target, then press ENTER.")
        print("    → Press ESC to skip this template.")

        while True:
            cv2.imshow(self.title, self.display)
            key = cv2.waitKey(30) & 0xFF

            if key == 13 and self.start and self.end:  # ENTER
                cv2.destroyAllWindows()
                x1 = int(min(self.start[0], self.end[0]) / scale)
                y1 = int(min(self.start[1], self.end[1]) / scale)
                x2 = int(max(self.start[0], self.end[0]) / scale)
                y2 = int(max(self.start[1], self.end[1]) / scale)
                return (x1, y1, x2 - x1, y2 - y1)

            if key == 27:  # ESC
                cv2.destroyAllWindows()
                return None


# ── helpers ─────────────────────────────────────────────────

def grab_screen() -> np.ndarray:
    """Capture the primary monitor as a BGR numpy array."""
    with mss.MSS() as sct:
        raw = np.array(sct.grab(sct.monitors[1]))
        return cv2.cvtColor(raw, cv2.COLOR_BGRA2BGR)


def wait_for_key(key_name: str = "space"):
    """Block until the user presses a key."""
    if HAS_KEYBOARD:
        keyboard.wait(key_name)
        time.sleep(0.3)  # debounce
    else:
        input(f"    Press Enter (in this console) when ready...")


def capture_one(name: str, instructions: str) -> bool:
    """Guide the user through capturing a single template image."""

    print(f"\n{'━' * 62}")
    print(f"  📸  {name}")
    print(f"{'━' * 62}")
    for line in instructions.strip().splitlines():
        print(f"  {line}")
    print()
    prompt = "SPACE" if HAS_KEYBOARD else "Enter"
    print(f"  Press {prompt} when the correct game screen is visible...")

    wait_for_key()

    screenshot = grab_screen()
    sel = RectangleSelector(screenshot, f"Select: {name}")
    rect = sel.run()

    if rect is None:
        print(f"  ✗  Skipped — {name} was NOT saved.")
        return False

    x, y, w, h = rect
    cropped = screenshot[y : y + h, x : x + w]

    save_path = TEMPLATE_DIR / f"{name}.png"
    cv2.imwrite(str(save_path), cropped)
    print(f"  ✓  Saved: {save_path}  ({w}×{h} px)")
    return True


# ── main ────────────────────────────────────────────────────

TEMPLATES = [
    (
        "artifacts_tab_selected",
        "Open a character's screen and click the  ◆ Artifacts  tab so\n"
        "it is SELECTED (bold white text).\n"
        "You will crop JUST the 'Artifacts' text from the left sidebar.",
    ),
    (
        "artifacts_tab",
        "Now click any OTHER tab (e.g. Attributes) so that the\n"
        "Artifacts tab is NOT selected (dimmed text).\n"
        "You will crop the dimmed 'Artifacts' text from the left sidebar.",
    ),
    (
        "remove_button",
        "Go to the Artifacts tab, click an equipped artifact to open\n"
        "its detail view.  You should see 'Remove' and 'Reshape'\n"
        "buttons at the bottom-right.\n"
        "You will crop JUST the 'Remove' button.",
    ),
    (
        "slot_filled",
        "(Optional) Go to the Artifacts tab with at least one equipped\n"
        "artifact visible.  You will crop the golden glow CIRCLE\n"
        "around one artifact.  Try to capture the outer ring, not the\n"
        "icon inside.",
    ),
]


def main():
    print("=" * 62)
    print("  Genshin Impact — Template Capture Utility")
    print("  Creates reference images for the batch artifact remover.")
    print("=" * 62)
    print()
    print("  Make sure Genshin Impact is open and visible.")
    print("  You will capture 4 small UI elements one at a time.")
    print()

    ok = 0
    for name, desc in TEMPLATES:
        if capture_one(name, desc):
            ok += 1

    print(f"\n{'=' * 62}")
    print(f"  Done!  {ok}/{len(TEMPLATES)} templates captured.")
    print(f"  Saved to:  {TEMPLATE_DIR.resolve()}")
    print(f"{'=' * 62}")


if __name__ == "__main__":
    main()
