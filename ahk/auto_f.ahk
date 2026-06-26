#NoEnv
SendMode Input
SetWorkingDir %A_ScriptDir%

; F8 to toggle the auto-presser on and off
Toggle := 0

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
    Send, f
return

RemoveToolTip:
    ToolTip
return
