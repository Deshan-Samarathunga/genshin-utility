# Template Images

This folder holds small cropped PNG screenshots used by `batch_artifact_remover.py`
for OpenCV template matching.

## Required Templates

| File | What to capture |
|------|-----------------|
| `artifacts_tab_selected.png` | The "◆ **Artifacts**" text when the tab is **active** (bold/highlighted) |
| `artifacts_tab.png` | The "◆ Artifacts" text when the tab is **inactive** |
| `remove_button.png` | The "Remove" button at the bottom-right of the artifact detail view |
| `slot_filled.png` | *(Optional)* The golden glow ring around an equipped artifact |

## How to Capture

Run the interactive capture utility:

```
python capture_templates.py
```

Or manually crop from a full-resolution (1920×1080) screenshot.
Make sure the crops are tight — include only the element itself with minimal surrounding area.
