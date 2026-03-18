#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; often works better in games
CoordMode "Mouse", "Screen"

; Only active while Genshin is active
#HotIf WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")

F6:: {
    MouseGetPos &ox, &oy
    DllCall("SetCursorPos", "int", 1552, "int", 1121)  ; your coords
    Click
    DllCall("SetCursorPos", "int", ox, "int", oy)
}

#HotIf