#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; often works better in games
CoordMode "Mouse", "Screen"

; Change this if needed.
; DualSense touchpad click is often Joy14, but it can vary by driver.
L3Button := 14

; Set these to the on-screen mic button position for Win+H voice typing.
MicX := 0
MicY := 0

; Set to true to use keyboard navigation instead of mouse coordinates.
UseTabMode := true
MicTabCount := 4

; Long-press settings (hold the button to clear chat).
LongPressMs := 2000
ClearChatUseSelectAll := true
ClearChatBackspaces := 40

L3State := 0
L3Hotkey := "Joy" . L3Button
L3IsDown := false
LongPressFired := false
L3DownStart := 0

IsGenshinActive(*) {
    return WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")
}

HotIf(IsGenshinActive)
Hotkey L3Hotkey, HandleDown
Hotkey L3Hotkey . " Up", HandleUp
HotIf()

HandleDown(*) {
    global L3IsDown, LongPressFired, LongPressMs, L3DownStart

    if (L3IsDown) {
        return
    }

    L3IsDown := true
    LongPressFired := false
    L3DownStart := A_TickCount
    SetTimer CheckLongPress, 50
}

HandleUp(*) {
    global L3IsDown, LongPressFired

    L3IsDown := false
    SetTimer CheckLongPress, 0

    if (LongPressFired) {
        LongPressFired := false
        return
    }

    HandleShortPress()
}

HandleShortPress() {
    global L3State, MicX, MicY, UseTabMode, MicTabCount

    switch L3State {
        case 0:
            Send "#h"
            L3State := 1
        case 1:
            if (UseTabMode) {
                Sleep 150 ; allow Win+H panel to appear
                Send "{Tab " . MicTabCount . "}{Space}"
            } else {
                if (MicX <= 0 || MicY <= 0) {
                    SoundBeep 1500, 150
                    return
                }
                MouseGetPos &ox, &oy
                DllCall("SetCursorPos", "int", MicX, "int", MicY)
                Click
                DllCall("SetCursorPos", "int", ox, "int", oy)
            }
            L3State := 2
        default:
            Send "#h"
            L3State := 0
    }
}

CheckLongPress() {
    global L3IsDown, LongPressFired, LongPressMs, L3DownStart
    global ClearChatUseSelectAll, ClearChatBackspaces

    if (!L3IsDown || LongPressFired) {
        SetTimer CheckLongPress, 0
        return
    }

    if ((A_TickCount - L3DownStart) < LongPressMs) {
        return
    }

    LongPressFired := true
    SetTimer CheckLongPress, 0

    if (ClearChatUseSelectAll) {
        Send "^a{Backspace}"
    } else {
        Send "{Backspace " . ClearChatBackspaces . "}"
    }
}
