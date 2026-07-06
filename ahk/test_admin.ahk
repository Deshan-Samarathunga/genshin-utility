#Requires AutoHotkey v2.0
if not A_IsAdmin {
    try Run '*RunAs "' A_AhkPath '" "' A_ScriptFullPath '"'
    ExitApp
}
MsgBox "I am Admin!"
