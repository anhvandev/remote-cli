# PROTOTYPE: types "a" through a fresh uinput keyboard (needs root + python3-evdev).
import time
from evdev import UInput, ecodes as e

ui = UInput({e.EV_KEY: [e.KEY_A]}, name="lockscreen-probe")
time.sleep(1)  # give the compositor time to pick up the new device
ui.write(e.EV_KEY, e.KEY_A, 1); ui.write(e.EV_KEY, e.KEY_A, 0); ui.syn()
time.sleep(0.2)
ui.close()
