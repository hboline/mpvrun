from pywinctl import getWindowsWithTitle, Re

def main():
    while True:
        try:
            handle = getWindowsWithTitle('mpv', condition=Re.CONTAINS)[0]
        except IndexError:
            continue
        else:
            break
    old = handle.position
    while True:
        try:
            new = handle.position
        except Exception:
            exit()
        if old != new:
            old = new
            handle.alwaysOnTop(False)
            handle.alwaysOnTop()
       
if __name__ == "__main__":
    main()
