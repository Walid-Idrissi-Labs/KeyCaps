"""sounds.py RECORDING EXPECTED: checks a recording of what the sound server played
(raw signed 16-bit stereo at 48 kHz) against the sounds expected, in order and separated
by spaces: L (louder on the left), R (on the right), C (in the middle), S (anything, but
shorter than 200 ms). An empty EXPECTED means silence. A third argument, "even", also
asks for every sound to be as loud as the others, within 1 dB."""
import struct, sys

data = open(sys.argv[1], "rb").read()
frames = struct.unpack(f"<{len(data) // 2}h", data[: len(data) // 2 * 2])
left, right = frames[0::2], frames[1::2]
# A sound: 1 ms windows above -64 dB, not more than 120 ms apart. (Keycaps plays 18 dB
# below its menu volume whatever the system volume, so this is -46 dB from that.)
window, sounds, quiet = 48, [], 10**9
for at in range(0, len(left) - window, window):
    l, r = max(map(abs, left[at:at + window])), max(map(abs, right[at:at + window]))
    if max(l, r) < 21:
        quiet += 1
        continue
    if quiet >= 120:
        sounds.append([at, at, 0, 0])
    quiet = 0
    sound = sounds[-1]
    sound[1:] = at + window, max(sound[2], l), max(sound[3], r)

def kind(sound):
    start, end, l, r = sound
    return "L" if l > 1.25 * r else "R" if r > 1.25 * l else "C"

expected = sys.argv[2].split()
got = [kind(s) for s in sounds]
for start, end, l, r in sounds:
    print(f"    at {start / 48:7.0f} ms for {(end - start) / 48:4.0f} ms, left {l / 32768:.3f} right {r / 32768:.3f}")
ok = len(got) == len(expected) and all(e == g or (e == "S" and (s[1] - s[0]) < 200 * 48) for e, g, s in zip(expected, got, sounds))
peaks = [max(s[2:]) for s in sounds]
if sys.argv[3:] == ["even"] and peaks and max(peaks) > 1.12 * min(peaks):
    ok = False
    print("    EXPECTED as loud as each other, within 1 dB")
print(f"    heard: {' '.join(got) or 'nothing'}" + ("" if ok else f"   EXPECTED: {' '.join(expected) or 'nothing'}"))
sys.exit(0 if ok else 1)
