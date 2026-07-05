#Requires AutoHotkey v2.0
#SingleInstance Force
SendMode "Event"          ; Event mode is the most reliable for Genshin's text fields

; =====================================================================
;  Genshin Impact - Keyboard-less Voice Chat  (PS5 DualSense + AHK v2)
; =====================================================================
;  WHAT IT DOES
;    Dictate and edit Genshin chat messages using ONLY the DualSense.
;    One button toggles Windows Voice Typing (Win+H) so you can speak
;    your message; other buttons insert spaces, delete characters,
;    clear the box, and send the message - no keyboard ever touched.
;
;  HOW TO USE IN-GAME
;    1. Open Genshin's chat box (co-op chat).
;    2. Press the VOICE button -> speak -> press VOICE again to stop.
;    3. Fix mistakes with SPACE / BACKSPACE / CLEAR.
;    4. Press SUBMIT to send.
;
;  >>> READ THIS ONCE (important real-world limits) <<<
;    * AHK reads the pad through DirectInput. It CANNOT block those
;      presses from ALSO reaching Genshin. Whatever button you map
;      below will still do its normal in-game action. So map buttons
;      that are harmless while the chat box is open (Touchpad, Options,
;      Create/Share, stick-clicks) - NOT movement/attack buttons.
;    * Windows "Voice typing" must be enabled (Settings > the Win+H
;      panel). On modern Win11 the panel starts listening automatically;
;      if yours does not, set AutoStartMic := true below.
;    * If you route the pad through Steam Input / DS4Windows as an
;      Xbox pad, the button NUMBERS change. Use the built-in tester
;      (press Ctrl+Alt+J) to find the right numbers for YOUR setup.
; =====================================================================


; #####################################################################
; ##  1) CONTROLLER BUTTON MAP   <-- EDIT THESE NUMBERS TO REMAP      ##
; #####################################################################
;
;  DualSense button numbers (typical DirectInput, no Steam/DS4Windows):
;    Joy1  = Square        Joy2  = Cross (X)     Joy3  = Circle
;    Joy4  = Triangle      Joy5  = L1            Joy6  = R1
;    Joy7  = L2 (button)   Joy8  = R2 (button)   Joy9  = Create/Share
;    Joy10 = Options       Joy11 = L3 (L-stick)  Joy12 = R3 (R-stick)
;    Joy13 = PS button     Joy14 = Touchpad click
;    D-Pad = POV hat (an angle, not a numbered button)
;
;  Defaults below use only the "chat-safe" buttons (non-combat).
;  To remap: change the number to match the table above (or whatever
;  the Ctrl+Alt+J tester shows for your controller).

Btn_Voice     := 14   ; Touchpad click   -> start/stop voice dictation
Btn_Space     := 11   ; L3 (left stick)  -> insert a space
Btn_Backspace := 12   ; R3 (right stick) -> delete last character (hold to repeat)
Btn_Clear     := 9    ; Create / Share   -> clear the whole chat box
Btn_Submit    := 10   ; Options          -> send the message (Enter)


; #####################################################################
; ##  2) BEHAVIOUR OPTIONS                                           ##
; #####################################################################

VoiceMode := "Toggle"        ; "Toggle"     = press once to start, again to stop
                             ; "PushToTalk" = hold to dictate, release to stop

ClearUseSelectAll := false   ; false = send a burst of Backspaces instead (Genshin does not support Ctrl+A)
                             ; true  = Ctrl+A then Backspace (fast, clean)
ClearBackspaces   := 60      ; used only when ClearUseSelectAll = false

StopVoiceOnSubmit := true    ; close the Win+H panel just before sending

; --- Backspace auto-repeat (hold to delete many characters) ---
BackspaceRepeatDelay := 350  ; ms to hold before auto-repeat starts (0 = off)
BackspaceRepeatMs    := 120  ; ms between repeats while held

; --- Win+H mic auto-start fallback ---
;   Leave false on modern Win11 (the panel auto-listens). If yours does
;   not start listening, set true and tune MicTabCount so Tab lands on
;   the mic/start button inside the voice typing panel.
AutoStartMic := false
MicTabCount  := 4

PollMs := 25                 ; controller poll rate in ms (lower = snappier)


; #####################################################################
; ##  3) ENGINE  (no need to edit below for normal remapping)        ##
; #####################################################################

; Genshin window check - this is the v2 equivalent of #IfWinActive.
; Every action is gated through here, so the script never disturbs
; normal Windows use; it only acts while Genshin is the active window.
IsGenshinActive(*) {
    return WinActive("ahk_exe GenshinImpact.exe")   ; Global client
        || WinActive("ahk_exe YuanShen.exe")        ; CN client
}

VoiceOn := false

; One state object per mapped action: which button, and edge-tracking.
Actions := Map(
    "voice",     { num: Btn_Voice,     down: false, downAt: 0, lastRepeat: 0 },
    "space",     { num: Btn_Space,     down: false, downAt: 0, lastRepeat: 0 },
    "backspace", { num: Btn_Backspace, down: false, downAt: 0, lastRepeat: 0 },
    "clear",     { num: Btn_Clear,     down: false, downAt: 0, lastRepeat: 0 },
    "submit",    { num: Btn_Submit,    down: false, downAt: 0, lastRepeat: 0 }
)

SetTimer Poll, PollMs
TrayTip "Ctrl+Alt+J = button tester. Active only while Genshin is focused.", "Genshin Voice Chat loaded", 1

; --- Main loop: poll every mapped button, fire on the right edge ---
Poll() {
    global Actions
    ; Only act when Genshin is in front. When it is not, clear the
    ; "down" flags so we don't fire a stale release/press on return.
    if !IsGenshinActive() {
        for name, b in Actions
            b.down := false
        return
    }

    for name, b in Actions {
        pressed := GetKeyState("Joy" . b.num)
        if (pressed && !b.down) {            ; ----- rising edge: press
            b.down := true
            b.downAt := A_TickCount
            b.lastRepeat := A_TickCount
            OnPress(name)
        } else if (!pressed && b.down) {     ; ----- falling edge: release
            b.down := false
            OnRelease(name)
        } else if (pressed && b.down) {      ; ----- still held
            OnHold(name, b)
        }
    }
}

OnPress(name) {
    global VoiceMode
    switch name {
        case "voice":
            (VoiceMode = "PushToTalk") ? StartVoice() : ToggleVoice()
        case "space":
            Send "{Space}"
        case "backspace":
            Send "{Backspace}"
        case "clear":
            ClearChat()
        case "submit":
            SubmitChat()
    }
}

OnRelease(name) {
    global VoiceMode
    if (name = "voice" && VoiceMode = "PushToTalk")
        StopVoice()
}

OnHold(name, b) {
    global BackspaceRepeatDelay, BackspaceRepeatMs
    ; Only Backspace auto-repeats while held.
    if (name != "backspace" || BackspaceRepeatDelay <= 0)
        return
    if (A_TickCount - b.downAt < BackspaceRepeatDelay)
        return
    if (A_TickCount - b.lastRepeat >= BackspaceRepeatMs) {
        b.lastRepeat := A_TickCount
        Send "{Backspace}"
    }
}

; --- Voice typing (Win+H) helpers ---
ToggleVoice() {
    global VoiceOn
    VoiceOn ? StopVoice() : StartVoice()
}

StartVoice() {
    global VoiceOn, AutoStartMic, MicTabCount
    if (VoiceOn)
        return
    Send "#h"                 ; opens (and on Win11, starts) Voice Typing
    VoiceOn := true
    if (AutoStartMic) {
        Sleep 400             ; wait for the panel to appear
        Send "{Tab " . MicTabCount . "}{Space}"   ; click the mic button
    }
}

StopVoice() {
    global VoiceOn
    if (!VoiceOn)
        return
    Send "#h"                 ; Win+H again closes the panel
    VoiceOn := false
}

; --- Editing helpers ---
ClearChat() {
    global ClearUseSelectAll, ClearBackspaces
    if (ClearUseSelectAll)
        Send "^a{Backspace}"
    else
        Send "{Backspace " . ClearBackspaces . "}"
}

SubmitChat() {
    global StopVoiceOnSubmit, VoiceOn
    if (StopVoiceOnSubmit && VoiceOn) {
        StopVoice()
        Sleep 120             ; let focus settle back on the chat box
    }
    Send "{Enter}"
}


; #####################################################################
; ##  4) SETUP HELPER: button tester  (keyboard - setup only)        ##
; #####################################################################
;  Press Ctrl+Alt+J, then press DualSense buttons to see their numbers.
;  Use this to fill in the button map in section 1. Press Ctrl+Alt+J
;  again to close. (Keyboard is fine here - it's a one-time setup tool.)

TesterOn := false
^!j:: {
    global TesterOn
    TesterOn := !TesterOn
    if (TesterOn)
        SetTimer ShowJoy, 100
    else {
        SetTimer ShowJoy, 0
        ToolTip
    }
}

ShowJoy() {
    s := "DualSense tester  (Ctrl+Alt+J to close)`n"
    s .= "D-Pad / POV angle: " . GetKeyState("JoyPOV") . "`n"
    Loop 32 {
        if GetKeyState("Joy" . A_Index)
            s .= "  Joy" . A_Index . "  DOWN`n"
    }
    ToolTip s
}
