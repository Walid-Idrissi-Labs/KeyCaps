"""A virtual keyboard and mouse, made with /dev/uinput (so: run as root). Commands come on
stdin, one per line, and each is answered with a line on stdout:

    key CODE        press and release a key (a Linux key code)
    hold CODE MS    press, let the kernel-style repeats run for MS, release
    click BUTTON    0x110 left, 0x111 right, 0x112 middle
    move DX DY      move the pointer
    wheel N         turn the wheel
    sleep MS
    open            let every user read the two devices (they start readable by root only)
    unplug / plug   remove the devices, and make them again
"""
import fcntl, glob, os, struct, sys, time

UI_SET_EVBIT, UI_SET_KEYBIT, UI_SET_RELBIT = 0x40045564, 0x40045565, 0x40045566
UI_DEV_SETUP, UI_DEV_CREATE, UI_DEV_DESTROY, UI_GET_SYSNAME = 0x405C5503, 0x5501, 0x5502, 0x8040552C
EV_SYN, EV_KEY, EV_REL = 0, 1, 2

def device(name, keys, axes=()):
    fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
    for key in keys:
        fcntl.ioctl(fd, UI_SET_KEYBIT, key)
    if axes:
        fcntl.ioctl(fd, UI_SET_EVBIT, EV_REL)
        for axis in axes:
            fcntl.ioctl(fd, UI_SET_RELBIT, axis)
    fcntl.ioctl(fd, UI_DEV_SETUP, struct.pack("HHHH80sI", 3, 0x1234, 0x5678, 1, name.encode(), 0))
    fcntl.ioctl(fd, UI_DEV_CREATE)
    return fd

def nodes(fd):
    """The /dev/input/eventN of a device."""
    name = fcntl.ioctl(fd, UI_GET_SYSNAME, bytes(64)).split(b"\0")[0].decode()
    return ["/dev/input/" + os.path.basename(p) for p in glob.glob(f"/sys/devices/virtual/input/{name}/event*")]

def emit(fd, kind, code, value):
    event = lambda kind, code, value: struct.pack("llHHi", 0, 0, kind, code, value)
    os.write(fd, event(kind, code, value) + event(EV_SYN, 0, 0))

def plug():
    made = device("Keycaps test keyboard", range(1, 249)), device("Keycaps test mouse", [0x110, 0x111, 0x112], [0, 1, 8])
    time.sleep(0.3)  # until the kernel has made the nodes
    return made

keyboard, mouse = plug()
print("ready", flush=True)
for line in sys.stdin:
    word = line.split()
    number = [int(w, 0) for w in word[1:]]
    if word[0] == "key":
        emit(keyboard, EV_KEY, number[0], 1); time.sleep(0.03); emit(keyboard, EV_KEY, number[0], 0)
    elif word[0] == "hold":
        emit(keyboard, EV_KEY, number[0], 1); time.sleep(0.05)
        for _ in range(number[1] // 30):
            emit(keyboard, EV_KEY, number[0], 2); time.sleep(0.03)
        emit(keyboard, EV_KEY, number[0], 0)
    elif word[0] == "click":
        emit(mouse, EV_KEY, number[0], 1); time.sleep(0.03); emit(mouse, EV_KEY, number[0], 0)
    elif word[0] == "move":
        emit(mouse, EV_REL, 0, number[0]); emit(mouse, EV_REL, 1, number[1])
    elif word[0] == "wheel":
        emit(mouse, EV_REL, 8, number[0])
    elif word[0] == "sleep":
        time.sleep(number[0] / 1000)
    elif word[0] == "open":
        for node in nodes(keyboard) + nodes(mouse):
            os.chmod(node, 0o666)
        time.sleep(0.3)
    elif word[0] == "unplug":
        for fd in (keyboard, mouse):
            fcntl.ioctl(fd, UI_DEV_DESTROY); os.close(fd)
        time.sleep(0.3)
    elif word[0] == "plug":
        keyboard, mouse = plug()
    print("did", line.strip(), flush=True)
