# Synthetic preview fixtures

Generated locally with FFmpeg 9.0.2 for plan0050; no user content. These files
are embedded only in the `validation` feature, then imported through real core
attachment encryption. Builds consume the checked-in bytes and need no FFmpeg.
The 8 MiB silent WAV is generated as a bounded reader at runtime, within the existing10 MiB per-file limit.

- `preview.mp4`: four seconds of `testsrc=size=128x96:rate=12`, H.264/yuv420p,
  plus `sine=frequency=440:sample_rate=22050` at 1% amplitude, AAC, faststart,
  a keyframe every second (`-g 12`, plan 0057). This checks basic controls;
  the sparse-keyframe fixture below checks the retained-source replay fault.
  SHA-256: `8a37581fd0dd368b28b6b9c06d60d8331ba8f6dd6d6050a39b161e92d0968b59`.
- `preview-long-gop.mp4`: twelve seconds of moving `testsrc2` at 128×96,
  30 fps, H.264/yuv420p with B-frames and keyframes at 0 and 8.33 seconds,
  AAC at 1% amplitude, index at the end. Reproduces the replay freeze without
  using a personal recording. The native test checks video-frame progress in
  the first seconds of initial play and two replays, plus pause/resume and seek.
  After EOS, slider selections at 1 s and 0 must show the selected frame while
  paused and resume with moving frames when Play is pressed.
  SHA-256: `acefcfa07f56225cb15fd290280e244d5f95f162b95aa366e9dc3dae7441a4f7`.
- `preview.png`: one blue 64×32 RGB PNG.
  SHA-256: `86784280e71c3efcebfce00ccca654157d2d5b02778cc07daad8b7dd9d6eef5a`.
- `large-preview.png`: one black 5000×4000 RGB PNG; its RGBA lower bound is
  80,000,000 bytes. The engine test verifies that it is deferred before decoding.
  SHA-256: `353a0ff9a999605ba015fbc326f57549a07657d089969341df07272bcbf4c3b9`.

Generation uses FFmpeg's `lavfi` sources, `-map_metadata -1`, and `-threads 1`.
The MP4 uses `-t 4 -c:v libx264 -g 12 -pix_fmt yuv420p -c:a aac -af volume=0.01
-movflags +faststart`; PNG sources use `color` and `-frames:v 1`.

The long-GOP fixture was generated with FFmpeg 9.0.2:

```sh
ffmpeg -f lavfi -i testsrc2=size=128x96:rate=30 \
  -f lavfi -i sine=frequency=440:sample_rate=48000 -t 12 \
  -c:v libx264 -threads 1 -g 250 -keyint_min 250 -sc_threshold 0 -bf 3 \
  -pix_fmt yuv420p -c:a aac -af volume=0.01 -map_metadata -1 preview-long-gop.mp4
```
