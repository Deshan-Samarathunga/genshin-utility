import ctypes
import time
import mss
import cv2
import numpy as np
import keyboard
import argparse

# --- Keyboard Input Setup for DirectX games (Genshin Impact) ---
PUL = ctypes.POINTER(ctypes.c_ulong)
class KeyBdInput(ctypes.Structure):
    _fields_ = [("wVk", ctypes.c_ushort),
                ("wScan", ctypes.c_ushort),
                ("dwFlags", ctypes.c_ulong),
                ("time", ctypes.c_ulong),
                ("dwExtraInfo", PUL)]
class HardwareInput(ctypes.Structure):
    _fields_ = [("uMsg", ctypes.c_ulong),
                ("wParamL", ctypes.c_short),
                ("wParamH", ctypes.c_ushort)]
class MouseInput(ctypes.Structure):
    _fields_ = [("dx", ctypes.c_long),
                ("dy", ctypes.c_long),
                ("mouseData", ctypes.c_ulong),
                ("dwFlags", ctypes.c_ulong),
                ("time", ctypes.c_ulong),
                ("dwExtraInfo", PUL)]
class Input_I(ctypes.Union):
    _fields_ = [("ki", KeyBdInput),
                ("mi", MouseInput),
                ("hi", HardwareInput)]
class Input(ctypes.Structure):
    _fields_ = [("type", ctypes.c_ulong),
                ("ii", Input_I)]

KEYEVENTF_SCANCODE = 0x0008
KEYEVENTF_KEYUP = 0x0002

def press_key(hexKeyCode):
    extra = ctypes.c_ulong(0)
    ii_ = Input_I()
    ii_.ki = KeyBdInput(0, hexKeyCode, KEYEVENTF_SCANCODE, 0, ctypes.pointer(extra))
    x = Input(ctypes.c_ulong(1), ii_)
    ctypes.windll.user32.SendInput(1, ctypes.pointer(x), ctypes.sizeof(x))

def release_key(hexKeyCode):
    extra = ctypes.c_ulong(0)
    ii_ = Input_I()
    ii_.ki = KeyBdInput(0, hexKeyCode, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP, 0, ctypes.pointer(extra))
    x = Input(ctypes.c_ulong(1), ii_)
    ctypes.windll.user32.SendInput(1, ctypes.pointer(x), ctypes.sizeof(x))

# DirectInput scan codes for A,S,D,J,K,L
KEYS = {
    'A': 0x1E,
    'S': 0x1F,
    'D': 0x20,
    'J': 0x24,
    'K': 0x25,
    'L': 0x26
}

# --- Game Logic Setup ---
# Estimated screen coordinates for the 6 hit zones (1920x1080 resolution fullscreen)
# IF IT DOES NOT HIT THE NOTES: 
# Run the script, press 'C' to turn on Calibration Mode, and verify the boxes are
# precisely positioned over the 6 circles on the hit line. Adjust the X,Y values below.
# Format: (x_center, y_center)
HIT_ZONES = {
    'A': (414, 918),
    'S': (631, 918),
    'D': (848, 918),
    'J': (1063, 918),
    'K': (1282, 918),
    'L': (1497, 918)
}

ZONE_RADIUS = 25  # Use a 50x50 box

# Exact color matching based on user-provided hex codes
# Yellow notes (H=~20)
YELLOW_LOWER = np.array([15, 100, 150])
YELLOW_UPPER = np.array([35, 255, 255])

# Purple notes (H=~126)
PURPLE_LOWER = np.array([115, 60, 150])
PURPLE_UPPER = np.array([145, 255, 255])

# Volume thresholds (how many pixels must match to trigger a press)
# Increasing these makes the bot wait until the note is deeper inside the box (presses LATER)
YELLOW_THRESHOLD = 400
PURPLE_START_THRESHOLD = 400
PURPLE_HOLD_THRESHOLD = 1

# Anticipation offset (Pixels) - keep at 0 since we are using time delay now
ANTICIPATION_OFFSET = 0 

# Time delay (in seconds) between seeing the note and actually pressing the key
# 0.10 means it will wait 100 milliseconds for the note to fall "more down" into the center
PRESS_DELAY = 0.12 

def check_zone_counts(img, center):
    x, y = center
    # Box is TALL: Extends upwards by ANTICIPATION_OFFSET to see notes early, 
    # but extends downwards to the hit line to keep hold notes active.
    cropped = img[max(0, y - ZONE_RADIUS - ANTICIPATION_OFFSET) : y + ZONE_RADIUS, x - ZONE_RADIUS : x + ZONE_RADIUS]
    if cropped.size == 0:
        return 0, 0
        
    # Drop alpha channel from MSS BGRA image to ensure perfect color conversion
    if len(cropped.shape) == 3 and cropped.shape[2] == 4:
        cropped = cropped[:, :, :3]
        
    hsv = cv2.cvtColor(cropped, cv2.COLOR_BGR2HSV)
    
    yellow_mask = cv2.inRange(hsv, YELLOW_LOWER, YELLOW_UPPER)
    purple_mask = cv2.inRange(hsv, PURPLE_LOWER, PURPLE_UPPER)
    
    return cv2.countNonZero(yellow_mask), cv2.countNonZero(purple_mask)

def run_bot():
    print("=== Rhythm Bot Started ===")
    print("Press 'Q' to quit anytime.")
    print("Press 'C' to toggle visual calibration mode.")
    print("Make sure Genshin Impact is in focus and visible on your primary monitor!")
    print("--------------------------")
    
    sct = mss.mss()
    
    min_x = max(0, min(z[0] for z in HIT_ZONES.values()) - ZONE_RADIUS * 2)
    max_x = max(z[0] for z in HIT_ZONES.values()) + ZONE_RADIUS * 2
    
    # We subtract 400 to capture a massive area ABOVE the notes so you can watch them fall in the calibration window
    min_y = max(0, min(z[1] for z in HIT_ZONES.values()) - ZONE_RADIUS * 2 - max(ANTICIPATION_OFFSET, 400))
    max_y = max(z[1] for z in HIT_ZONES.values()) + ZONE_RADIUS * 4 # Increased space at the bottom
    
    monitor = {"top": min_y, "left": min_x, "width": max_x - min_x, "height": max_y - min_y}
    rel_zones = {k: (v[0] - min_x, v[1] - min_y) for k, v in HIT_ZONES.items()}

    active_holds = {k: False for k in KEYS}
    is_yellow_active = {k: False for k in KEYS} 
    
    # Scheduling dictionaries to delay the physical press
    pending_taps = {k: 0.0 for k in KEYS}
    pending_holds = {k: 0.0 for k in KEYS}
    tap_release_times = {k: 0.0 for k in KEYS} 
    
    calibration_mode = False
    last_c_press = 0

    try:
        while True:
            if keyboard.is_pressed('q'):
                print("Quitting...")
                break
                
            if keyboard.is_pressed('c') and time.time() - last_c_press > 0.5:
                calibration_mode = not calibration_mode
                last_c_press = time.time()
                if not calibration_mode:
                    cv2.destroyAllWindows()
                print(f"Calibration Mode: {'ON' if calibration_mode else 'OFF'}")

            img = np.array(sct.grab(monitor))
            current_time = time.time()
            
            for key_name, rel_center in rel_zones.items():
                y_count, p_count = check_zone_counts(img, rel_center)
                scan_code = KEYS[key_name]
                
                # --- HOLD NOTES (PURPLE) ---
                if active_holds[key_name]:
                    if p_count < PURPLE_HOLD_THRESHOLD:
                        release_key(scan_code)
                        active_holds[key_name] = False
                        print(f"[{current_time:.2f}] Released {key_name} (Purple trail ended)")
                else:
                    if p_count > PURPLE_START_THRESHOLD and pending_holds[key_name] == 0.0:
                        # Schedule the hold to start slightly later
                        pending_holds[key_name] = current_time + PRESS_DELAY
                        
                # --- TAP NOTES (YELLOW) ---
                if y_count > YELLOW_THRESHOLD:
                    if not is_yellow_active[key_name] and not active_holds[key_name] and pending_taps[key_name] == 0.0:
                        # Schedule the tap to occur slightly later
                        pending_taps[key_name] = current_time + PRESS_DELAY
                        is_yellow_active[key_name] = True
                else:
                    is_yellow_active[key_name] = False
                        
            # Execute scheduled actions
            for key_name in KEYS:
                # Execute scheduled taps
                if pending_taps[key_name] > 0 and current_time >= pending_taps[key_name]:
                    press_key(KEYS[key_name])
                    tap_release_times[key_name] = current_time + 0.05 
                    pending_taps[key_name] = 0.0
                    print(f"[{current_time:.2f}] Tapped {key_name} (Yellow note)")
                    
                # Execute scheduled holds
                if pending_holds[key_name] > 0 and current_time >= pending_holds[key_name]:
                    press_key(KEYS[key_name])
                    active_holds[key_name] = True
                    pending_holds[key_name] = 0.0
                    print(f"[{current_time:.2f}] Holding {key_name} (Purple note)")
                
                # Execute scheduled releases
                if tap_release_times[key_name] > 0 and current_time >= tap_release_times[key_name]:
                    release_key(KEYS[key_name])
                    tap_release_times[key_name] = 0.0
            
            if calibration_mode:
                debug_img = img.copy()
                for key_name, rel_center in rel_zones.items():
                    x, y = rel_center
                    y_count, p_count = check_zone_counts(img, rel_center)
                    
                    color = (255, 255, 255)
                    if active_holds[key_name]:
                        color = (255, 0, 255)
                    elif y_count > YELLOW_THRESHOLD:
                        color = (0, 255, 255)
                        
                    # Draw tall detection box
                    cv2.rectangle(debug_img, (x-ZONE_RADIUS, y-ZONE_RADIUS-ANTICIPATION_OFFSET), (x+ZONE_RADIUS, y+ZONE_RADIUS), color, 2)
                    
                    cv2.putText(debug_img, f"{key_name}", (x-10, y-ZONE_RADIUS-15), cv2.FONT_HERSHEY_SIMPLEX, 0.5, color, 1)
                    cv2.putText(debug_img, f"Y:{y_count}", (x-20, y+ZONE_RADIUS+15), cv2.FONT_HERSHEY_SIMPLEX, 0.4, (0, 255, 255), 1)
                    cv2.putText(debug_img, f"P:{p_count}", (x-20, y+ZONE_RADIUS+30), cv2.FONT_HERSHEY_SIMPLEX, 0.4, (255, 0, 255), 1)
                    
                    # --- ILLUMINATE WHEN KEY IS PHYSICALLY PRESSED ---
                    is_physically_pressed = active_holds[key_name] or (tap_release_times[key_name] > 0)
                    if is_physically_pressed:
                        # Draw a solid green circle and 'PRESSED' text BELOW the box where there's space
                        indicator_y = y + ZONE_RADIUS + 60
                        cv2.circle(debug_img, (x, indicator_y), 15, (0, 255, 0), -1) 
                        cv2.putText(debug_img, "PRESS", (x-22, indicator_y + 35), cv2.FONT_HERSHEY_SIMPLEX, 0.5, (0, 255, 0), 2)
                    
                cv2.imshow("Rhythm Bot Calibration (Close this window or press C to hide)", debug_img)
                cv2.waitKey(1)
                
            time.sleep(0.005)

    finally:
        for key_name, is_held in active_holds.items():
            if is_held:
                release_key(KEYS[key_name])
        cv2.destroyAllWindows()
        print("Bot safely shut down.")

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description="Genshin Impact Rhythm Bot")
    args = parser.parse_args()
    
    print("Welcome to Rhythm Bot.")
    print("Press ENTER to start the bot. Make sure the game is visible.")
    input()
    run_bot()
