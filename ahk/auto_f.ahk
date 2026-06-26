#NoEnv
SendMode Input
SetWorkingDir %A_ScriptDir%

; F8 to toggle the auto-presser on and off
Toggle := 0
; Only active while Genshin is active and Scroll Lock is on
#If (WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")) && GetKeyState("ScrollLock", "T")

F8::
    Toggle := !Toggle
    if (Toggle)
    {
        SetTimer, PressF, 1000
        ToolTip, Auto-F Started!
        SetTimer, RemoveToolTip, -2000
    }
    else
    {
        SetTimer, PressF, Off
        ToolTip, Auto-F Stopped!
        SetTimer, RemoveToolTip, -2000
    }
return

PressF:
    if ((WinActive("ahk_exe GenshinImpact.exe") || WinActive("ahk_exe YuanShen.exe")) && GetKeyState("ScrollLock", "T"))
    {
        SendInput, {f down}
        Sleep, 50
        SendInput, {f up}
    }
return

RemoveToolTip:
    ToolTip
return

#If
