#Requires AutoHotkey v2.0
#SingleInstance Force

step := 5  ; brightness step (0–100)

#PgUp:: ChangeBrightness(step)
#PgDn:: ChangeBrightness(-step)

ChangeBrightness(delta) {
    cur := GetBrightness()
    if (cur = "")
        return
    newB := Clamp(cur + delta, 0, 100)
    SetBrightness(newB)
}

GetBrightness() {
    wmi := ComObjGet("winmgmts:\\.\root\WMI")
    for mon in wmi.ExecQuery("SELECT * FROM WmiMonitorBrightness") {
        return mon.CurrentBrightness
    }
    return ""
}

SetBrightness(level) {
    wmi := ComObjGet("winmgmts:\\.\root\WMI")
    for m in wmi.ExecQuery("SELECT * FROM WmiMonitorBrightnessMethods") {
        m.WmiSetBrightness(1, level)  ; (timeoutSeconds, brightness)
    }
}

Clamp(val, min, max) {
    return val < min ? min : val > max ? max : val
}