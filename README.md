# Voice bench samples

Twelve-second excerpts from the offline mic-chain check (`src/voice/chain_check.rs`) for the voice
artifact fixes. Each scene has the unprocessed input (`1-input`), the chain before the fix
(`2-before`) and after it (`3-after`), both as sent: through the voice gate and an Opus round trip.
Loudness is normalised to -20 LUFS so the files compare by ear.

| Scene | What's in it |
|---|---|
| `household` | Male and female speakers; synthetic fan at 15 dB SNR plus dog barks and a crying baby |
| `everything` | Fan at 5 dB SNR, louder dog and baby, keyboard |
| `p232_vacuum` | Male speaker, vacuum cleaner at 5 dB SNR |
| `p257_keyboard` | Female speaker, keyboard typing at 10 dB SNR |
| `p257_quiet` | Female speaker, no noise: what the chain does to clean speech |

## Credits

- Speech: clean test set of the Noisy speech database (Valentini-Botinhao, University of
  Edinburgh, VCTK speakers p232 and p257), CC BY 4.0. https://doi.org/10.7488/ds/2117
- Noise: ESC-50 (Karol J. Piczak), CC BY-NC 3.0, clips 1-100032-A-0, 1-30226-A-0 (dog),
  1-187207-A-20, 1-211527-A-20, 1-22694-A-20 (crying baby), 1-137-A-32, 1-61212-A-32,
  1-94231-A-32 (keyboard), 1-100210-A-36, 1-19840-A-36, 2-106072-A-36 (vacuum).
  https://github.com/karolpiczak/ESC-50
- The fan is synthesised (band-limited noise plus blade-pass hum).
