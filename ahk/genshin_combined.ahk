#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; often works better in games
CoordMode "Mouse", "Screen"

; All hotkeys only work while Genshin is active and Scroll Lock is on
#HotIf IsGenshinActive() && GetKeyState("ScrollLock", "T")

WheelUp:: {
    ClickCharacterLeft()
}

WheelDown:: {
    ClickCharacterRight()
}

F6:: {
    EquipArtifact()
}

F8:: {
    UnlockArtifact()
}

#HotIf

ClickCharacterLeft() {
    ClickAt(71, 592)
}

ClickCharacterRight() {
    ClickAt(1840, 594)
}

EquipArtifact() {
    ClickAt(1552, 1121)
}

UnlockArtifact() {
    ClickAt(1555, 494)
}

ClickAt(x, y) {
    MouseGetPos &ox, &oy
    DllCall("SetCursorPos", "int", x, "int", y)
    Click
    DllCall("SetCursorPos", "int", ox, "int", oy)
}

IsGenshinActive() {
    return WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")
}
