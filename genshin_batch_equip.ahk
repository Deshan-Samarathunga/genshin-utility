#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; often works better in games
CoordMode "Mouse", "Screen"

#HotIf IsGenshinActive()

F7:: {
    ClickSequence([
        [89, 43],
        [1552, 1121],
        [195, 41],
        [1552, 1121],
        [309, 40],
        [1552, 1121],
        [415, 37],
        [1552, 1121],
        [525, 44],
        [1552, 1121],
    ], 80)
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
