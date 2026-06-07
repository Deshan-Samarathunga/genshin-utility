

#NoEnv
#SingleInstance Force
SetWorkingDir %A_ScriptDir%
CoordMode, Mouse, Window

; --- Configuration ---
; The layout supports both the Lyre and the Vintage Lyre (Guitar):
; - Lyre Upper Row:   High Notes (Q W E R T Y U)
; - Guitar Top Row:   Chords C, Dm, Em, F, G, Am, G7 (Q W E R T Y U)
; (Both instruments use the exact same keys and coordinates on screen)
;
; Set to True if you want the script to click the specific coordinates on screen.
; Set to False to use standard keyboard inputs (recommended for PC Genshin).
UseMouseClicks := False

; Delay in milliseconds between each note
NoteDelay := 150
; Delay for spaces/rests
RestDelay := 250

MsgBox, 64, Lyre Player, Genshin Lyre script loaded!`n`nPress F8 to play.`nPress F9 to stop.`nPress F10 to exit.`n`nCtrl+Up : Faster`nCtrl+Down : Slower

; --- Hotkeys ---
; F8 plays the song. You can paste your copied charts into the `song` variable below.
F8::
    ; A Thousand Years - Christina Perri
    song = 
    (LTrim Join
    EZ-EE-WM-EN-EE-WB-RV-E-QV-QW-EB-WB-QV-Q-QV-EWQ-QZ-Q-QZ-TRE-QN-H-TRE-WB-EJW-HV-HV-HV-J-QB-JB-SG-EZ-TE-EZ-TE-YTWQM-EN-TE-EN-TEYT-WB-Q-QW-RV-E-HV-QWQW-RB-E-W-WB-EZ-TE-EZ-TE-YTWQM-EN-TE-EN-TEYT-WB-Q-QW-RV-E-HV-QWQW-RB-E-W-WB-QZ-CBADG-VN-AF-BM-SG-ZCQ
    )
    PlaySong(song)
return

; F9 stops playing by reloading the script
F9::
    Reload
return

; F10 completely closes the script
F10::
    ExitApp
return

; --- Tempo Controls ---
; Ctrl + Up Arrow: Faster
^Up::
    NoteDelay := NoteDelay - 5
    if (NoteDelay < 1)
        NoteDelay := 1
    RestDelay := RestDelay - 8
    if (RestDelay < 1)
        RestDelay := 1
    ToolTip, Tempo: Faster (NoteDelay: %NoteDelay%ms)
    SetTimer, RemoveToolTip, -1500
return

; Ctrl + Down Arrow: Slower
^Down::
    NoteDelay := NoteDelay + 5
    RestDelay := RestDelay + 8
    ToolTip, Tempo: Slower (NoteDelay: %NoteDelay%ms)
    SetTimer, RemoveToolTip, -1500
return

RemoveToolTip:
    ToolTip
return

; --- Functions ---

PlaySong(notes) {
    global UseMouseClicks, NoteDelay, RestDelay
    
    Loop, Parse, notes
    {
        char := A_LoopField
        
        ; Ignore formatting characters
        if (char = "`n" || char = "`r")
            continue
            
        ; Spaces or dashes act as rests (pauses)
        if (char = " " || char = "-") {
            Sleep, %RestDelay%
            continue
        }
        
        if (UseMouseClicks) {
            PlayByClick(char)
        } else {
            ; Simulate a realistic keypress for the game
            SendInput, {%char% down}
            Sleep, 50
            SendInput, {%char% up}
        }
        
        Sleep, %NoteDelay%
    }
}

PlayByClick(note) {
    StringUpper, note, note
    x := 0, y := 0
    
    ; Upper
    if (note = "Q") {
        x := 449
        y := 674
    } else if (note = "W") {
        x := 617
        y := 674
    } else if (note = "E") {
        x := 786
        y := 674
    } else if (note = "R") {
        x := 954
        y := 674
    } else if (note = "T") {
        x := 1121
        y := 674
    } else if (note = "Y") {
        x := 1289
        y := 674
    } else if (note = "U") {
        x := 1455
        y := 674
    }
    ; Middle
    else if (note = "A") {
        x := 449
        y := 805
    } else if (note = "S") {
        x := 617
        y := 805
    } else if (note = "D") {
        x := 786
        y := 805
    } else if (note = "F") {
        x := 954
        y := 805
    } else if (note = "G") {
        x := 1121
        y := 805
    } else if (note = "H") {
        x := 1289
        y := 805
    } else if (note = "J") {
        x := 1455
        y := 805
    }
    ; Lower
    else if (note = "Z") {
        x := 449
        y := 932
    } else if (note = "X") {
        x := 617
        y := 932
    } else if (note = "C") {
        x := 786
        y := 932
    } else if (note = "V") {
        x := 954
        y := 932
    } else if (note = "B") {
        x := 1121
        y := 932
    } else if (note = "N") {
        x := 1289
        y := 932
    } else if (note = "M") {
        x := 1455
        y := 932
    }
    
    if (x != 0 and y != 0) {
        Click, %x%, %y%
    }
}
