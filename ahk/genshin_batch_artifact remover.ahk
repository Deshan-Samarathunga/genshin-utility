#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; often works better in games
CoordMode "Mouse", "Screen"

global StopLoop := false

#HotIf IsGenshinActive()

F7:: {
    global StopLoop := false

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

    ; Top-left panel items
    panelItems := [
        [89, 43],
        [195, 41],
        [309, 40],
        [415, 37],
        [525, 44],
    ]

    removeBtn := [1558, 1002]
    backBtn   := [1838, 35]
    nextBtn   := [1840, 536]

    while (!StopLoop) {
        ; Step 1: Click all artifact coordinates
        ClickSequence(artifacts, 80)
        Sleep 200

        ; Step 2: Click each panel item then remove
        for item in panelItems {
            if (StopLoop)
                break
            ClickAt(item[1], item[2])
            Sleep 80
            ClickAt(removeBtn[1], removeBtn[2])
            Sleep 80
        }

        if (StopLoop)
            break

        ; Step 3: Back button
        Sleep 500
        ClickAt(backBtn[1], backBtn[2])
        Sleep 1000

        ; Step 4: Next button (next character)
        ClickAt(nextBtn[1], nextBtn[2])
        Sleep 1000
    }
}

F8:: {
    global StopLoop := true
}

#HotIf

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

IsGenshinActive() {
    return WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")
}

ClickAt(x, y) {
    DllCall("SetCursorPos", "int", x, "int", y)
    Click
}