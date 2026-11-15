# Synthesises Wave's sound cues into assets/sounds/*.mp3 (needs `lame`): python3 tools/sound_cues.py
# They follow Discord's cues in length, register and direction (up for on and join, down for off and
# leave) so they read the same, but sit in E-flat pentatonic on thirds and sixths, and every note
# glides into its pitch: a soft "bloop", Wave's own timbre.
import math, os, struct, subprocess, tempfile, wave

RATE = 48000
NOTE = {"Bb2": 46, "Eb3": 51, "F3": 53, "G3": 55, "Bb3": 58, "C4": 60, "Eb4": 63, "F4": 65, "G4": 67,
        "Bb4": 70, "C5": 72, "Eb5": 75, "F5": 77, "G5": 79, "Bb5": 82, "C6": 84, "Eb6": 87}

# name: (notes, seconds between onsets, decay seconds, glide direction, peak, brightness)
CUES = {
    "mute":               (["Bb4", "G4"], 0.075, 0.09, -1, 0.42, 0.20),
    "unmute":             (["G4", "Bb4"], 0.075, 0.09, +1, 0.42, 0.20),
    "deafen":             (["G4", "Eb4", "Bb3"], 0.075, 0.12, -1, 0.42, 0.18),
    "undeafen":           (["Bb3", "Eb4", "G4"], 0.075, 0.12, +1, 0.42, 0.18),
    "user_join":          (["Eb4", "C5"], 0.085, 0.20, +1, 0.45, 0.22),
    "user_leave":         (["C4", "Eb3"], 0.085, 0.20, -1, 0.45, 0.18),
    "disconnect":         (["Bb4", "F4", "Bb3"], 0.11, 0.22, -1, 0.45, 0.18),
    "stream_started":     (["Eb3", "G3", "Bb3", "Eb4", "G4"], 0.065, 0.28, +1, 0.5, 0.22),
    "stream_ended":       (["G4", "Eb4", "Bb3", "Eb3"], 0.075, 0.28, -1, 0.5, 0.18),
    "stream_user_joined": (["Bb3", "Eb4", "F4"], 0.08, 0.20, +1, 0.45, 0.18),
    "stream_user_left":   (["F4", "C4", "Bb3"], 0.08, 0.20, -1, 0.45, 0.16),
    "message":            (["Eb5", "Bb5"], 0.055, 0.07, +1, 0.36, 0.25),
    "mention":            (["Bb5", "Eb6", "Bb5"], 0.06, 0.10, +1, 0.42, 0.30),
    "ptt_start":          (["Bb4"], 0.0, 0.045, +1, 0.40, 0.15),
    "ptt_stop":           (["F4"], 0.0, 0.055, -1, 0.40, 0.15),
}
# A small room so the tails don't stop dead: (delay seconds, gain).
ROOM = [(0.031, 0.16), (0.053, 0.11), (0.089, 0.07), (0.131, 0.04)]
GLIDE = 0.022   # each note slides into pitch over this long...
BEND = 0.06     # ...from this far off (a fraction of the pitch).


def hz(note):
    return 440 * 2 ** ((NOTE[note] - 69) / 12)


def render(notes, step, decay, glide, peak, bright):
    length = step * (len(notes) - 1) + decay * 5 + ROOM[-1][0]
    out = [0.0] * int(length * RATE)
    for i, note in enumerate(notes):
        f, start, phase = hz(note), int(i * step * RATE), 0.0
        # Later notes ring a little longer, so a phrase ends on its last note.
        tau = decay * (1 + 0.6 * i / max(1, len(notes) - 1))
        for n in range(int(tau * 5 * RATE)):
            t = n / RATE
            bend = 1 - glide * BEND * math.exp(-t / GLIDE * 3)
            phase += 2 * math.pi * f * bend / RATE
            env = min(1, t / 0.004) * math.exp(-t / tau)
            tone = math.sin(phase) + bright * math.exp(-t / (tau * 0.35)) * math.sin(2 * phase) \
                + 0.35 * bright * math.exp(-t / (tau * 0.2)) * math.sin(3 * phase)
            if start + n < len(out):
                out[start + n] += env * tone
    dry = out[:]
    for delay, gain in ROOM:
        d = int(delay * RATE)
        for n in range(d, len(out)):
            out[n] += dry[n - d] * gain
    top = max(abs(v) for v in out)
    end = max(n for n, v in enumerate(out) if abs(v) > top * 0.01)
    fade = int(0.01 * RATE)
    out = [v * peak / top * min(1, (end - n) / fade) for n, v in enumerate(out[:end])]
    return out


def main():
    root = os.path.join(os.path.dirname(__file__), "..", "assets", "sounds")
    os.makedirs(root, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for name, spec in CUES.items():
            samples = render(*spec)
            path = os.path.join(tmp, name + ".wav")
            with wave.open(path, "wb") as w:
                w.setnchannels(1)
                w.setsampwidth(2)
                w.setframerate(RATE)
                w.writeframes(b"".join(struct.pack("<h", int(v * 32767)) for v in samples))
            subprocess.run(["lame", "--quiet", "-V", "4", path, os.path.join(root, name + ".mp3")], check=True)
            print(f"{name}: {len(samples) / RATE * 1000:.0f}ms")


if __name__ == "__main__":
    main()
