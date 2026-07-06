#Requires AutoHotkey v2.0
#SingleInstance Force

if not A_IsAdmin {
    try Run '*RunAs "' A_AhkPath '" "' A_ScriptFullPath '"'
    ExitApp
}

; -----------------------------------------------------------------------------
; Genshin Impact - Auto Dialogue / Custom Looter
; Trigger: F4 (press once to start, press again to stop)
; While running, it presses F and Space every second to advance dialogue / loot.
; -----------------------------------------------------------------------------

Toggle := false

$F4:: {
    global Toggle
    Toggle := !Toggle
    if (Toggle) {
        SetTimer LootLoop, 1000  ; run every 1000 ms
        LootLoop()               ; and run once immediately
    } else {
        SetTimer LootLoop, 0     ; turn the timer off
    }
}

LootLoop() {
    Send "f"
    Sleep 50
    Send "{Space}"
}
