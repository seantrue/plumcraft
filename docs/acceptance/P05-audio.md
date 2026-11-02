# P05: the audio engine in the editor (F)

*2026-10-27. PlumCraft's audio engine (46 nodes on SoundCraft's DSP and I/O crates) in the editor: waveforms on the nodes, a spectrogram and playback in the Watched panel. The first engine whose values aren't text.*

```sh
cd ~/workspace/plumcraft
cargo build                                            # builds plumcraft-audio-engine too
cargo run -p plumcraft-editor -- --engine audio        # in-process; or --process, --http
```

## Checklist

| # | Try | Expect | Result |
|---|---|---|---|
| 1 | Open it and **Run** | The demo: Tone → Fade → Gain → Plate Reverb → Loudness, Peak, Spectrum; teal waveform thumbs on the audio nodes (the fade-out and the reverb tail visible) | |
| 2 | Click Plate Reverb's title | Watched: "audio 48000 Hz, 2 ch, 5.37 s", a two-lane waveform with a time axis, a spectrogram | |
| 3 | **Play** (then Loop, Stop) | You hear a saw tone through a reverb; the playhead moves; a click on the waveform seeks | |
| 4 | Change Tone's frequency or waveform, Gain's dB, the reverb's Mix | Only the changed node and downstream recompute; the sound and pictures follow | |
| 5 | Watch Loudness and Peak | The LUFS and dBFS numbers | |
| 6 | Right-click: add `audio.load` (Sources), pick a file of yours (wav, mp3, flac, …), wire it into Gain | Your audio plays through the chain | |
| 7 | Add an effect from the palette (Audio/Effects: compressor, delay, chorus, EQ, …), wire it in | Its parameters are widgets; you hear it | |
| 8 | Add `audio.save` at the end, pick a path, run | A WAV appears there; it doesn't overwrite an existing file unless `overwrite` is on | |
| 9 | Set Tone to 60 s and Time Stretch ×4 on it, run, then **Cancel** | The run stops within about half a second | |
| 10 | `--process` and `--http` | The same, with the engine as a separate process or over HTTP | |
| 11 | Anything that looks or sounds wrong, slow, or confusing | Note it here | |

## Results

- 2026-10-27, the maintainer, all items in a dialog: 1–10 work (demo, Watched audio view, the listen test, recompute on edits, Loudness and Peak, their own file through the chain, an effect, saving a WAV, cancel of a long stretch, and the process and HTTP modes). P05 accepted.
