#SingleInstance Force

; -----------------------------------------------------------------------------
; Genshin Impact Custom Looter
; Trigger Button: F4 (Press once to start, press again to stop)
; -----------------------------------------------------------------------------

$F4::
    Toggle := !Toggle
    if (Toggle) {
        SetTimer, LootLoop, 1000  ; Sets a timer to run every 1000ms
        Gosub, LootLoop           ; Runs immediately the first time
    } else {
        SetTimer, LootLoop, Off   ; Turns the timer off when you press F4 again
    }
return

LootLoop:
    ; Press F
    Send, f
    Sleep, 50
    
    ; Press Space
    Send, {Space}
return
