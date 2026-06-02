#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; often works better in games
CoordMode "Mouse", "Screen"

#HotIf IsGenshinActive()

; ── F7: Remove artifacts from current character ──
F7:: {
    ; Artifact grid coordinates (click all at once to select)
    artifacts := [
        [1335, 735],
        [967, 787],
        [682, 780],
        [397, 638],
        [555, 558],
        [722, 771],
        [804, 784],
        [1222, 765],
    ]

    ; Top-left panel items (artifact slots)
    panelItems := [
        [89, 43],
        [195, 41],
        [309, 40],
        [415, 37],
        [525, 44],
    ]

    removeBtn := [1558, 1002]
    backBtn   := [1838, 35]

    ; Step 1: Click all artifact coordinates
    ClickSequence(artifacts, 80)
    Sleep 200

    ; Step 2: Click each panel item then remove
    for item in panelItems {
        ClickAt(item[1], item[2])
        Sleep 200
        ClickAt(removeBtn[1], removeBtn[2])
        Sleep 200
    }

    ; Step 3: Back button
    Sleep 500
    ClickAt(backBtn[1], backBtn[2])
    Sleep 1000

    ; Step 4: Next button
    nextBtn := [1840, 536]
    ClickAt(nextBtn[1], nextBtn[2])
}

; ── Left Arrow: Previous character ──
Left:: {
    ClickAt(68, 535)
}

; ── Right Arrow: Next character ──
Right:: {
    ClickAt(1840, 536)
}

#HotIf

; ────────────────────────────────────────
;  Helper functions
; ────────────────────────────────────────

ClickSequence(points, clickDelay := 120) {
    MouseGetPos &ox, &oy

    try {
        for point in points {
            DllCall("SetCursorPos", "int", point[1], "int", point[2])
            Click
            Sleep clickDelay
        }
    } finally {
        DllCall("SetCursorPos", "int", ox, "int", oy)
    }
}

ClickAt(x, y) {
    DllCall("SetCursorPos", "int", x, "int", y)
    Click
}

IsGenshinActive() {
    return WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")
}