#Requires AutoHotkey v2.0
#SingleInstance Force

if not A_IsAdmin {
    ArgsString := ""
    for arg in A_Args {
        ArgsString .= ' "' arg '"'
    }
    try Run '*RunAs "' A_AhkPath '" "' A_ScriptFullPath '"' ArgsString
    ExitApp
}

; Get parameters passed from Tauri
MessageText := ""
LoopCount := 1

if (A_Args.Length >= 1) {
    MessageText := A_Args[1]
}
if (A_Args.Length >= 2) {
    LoopCount := A_Args[2]
}

; Function to perform the messaging loop
RunMessagingLoop() {
    global MessageText, LoopCount
    
    ; Convert LoopCount to an integer
    try {
        count := Integer(LoopCount)
    } catch {
        count := 1
    }

    Loop count {
        ; 1. Click chat icon upper right of screen
        Click 1726, 166
        Sleep 500
        
        ; 2. Select chat
        Click 441, 1001
        Sleep 500
        
        ; 3. Paste the text
        ; Using clipboard is more reliable for games
        SavedClip := A_Clipboard
        A_Clipboard := MessageText
        Sleep 100
        Send "^v"
        Sleep 200
        A_Clipboard := SavedClip
        
        ; 4. Send button
        Click 1048, 1008
        Sleep 500
        
        ; 5. Chat close button
        Click 40, 42
        Sleep 500
        
        ; 6. Scroll 7 ticks to open next chat
        ; Assuming we are hovering over the chat list or we need to move mouse there first
        ; Let's move the mouse to the chat list area (left side) before scrolling
        MouseMove 200, 500
        Sleep 100
        Send "{WheelDown 7}"
        Sleep 500
    }
}

; Hotkey to trigger the loop (F8)
$F8:: {
    RunMessagingLoop()
}
