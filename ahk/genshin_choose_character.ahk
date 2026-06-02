#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; often works better in games
CoordMode "Mouse", "Screen"

; Only active while Genshin is active and Scroll Lock is on
#HotIf (WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")) && GetKeyState("ScrollLock", "T")

WheelUp:: {
    ClickCharacterLeft()
}

WheelDown:: {
    ClickCharacterRight()
}

#HotIf

ClickCharacterLeft() {
    ClickAt(71, 592)  ; first button
}

ClickCharacterRight() {
    ClickAt(1840, 594)  ; second button 
}

ClickAt(x, y) {
    MouseGetPos &ox, &oy
    DllCall("SetCursorPos", "int", x, "int", y)
    Click
    DllCall("SetCursorPos", "int", ox, "int", oy)
}
