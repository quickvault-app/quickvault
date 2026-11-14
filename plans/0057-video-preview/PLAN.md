# Plan 0057 — Video preview: playback stalls, player size, first frame

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| In progress — implementation done; full gates and fresh-context review pending | 2026-11-14 | 2026-11-14 (owner bug report and request; design delegated by the owner, standing 2026-11-02) | 0003 §6.4 (amended), 0009 §1.8 (amended) |

## Intent

The owner reported, for a 9.8 MB screen recording previewed in the Linux app:

1. before play, the player is very small;
2. sometimes play shows only a broken-video symbol;
3. when it plays, the picture stays on the first frame; seeking shows later
   frames but playback does not advance;
4. verification and streaming of large videos could be more efficient;
5. the first frame could load before play.

They also asked whether the loopback listener used for media is needed, and
want the pros and cons before deciding (see *Open question*).

An MP4 brand fix (`iso4` major brand, `ftyp` compatible brands) for the same
file preceded this plan; it is in the Unreleased changelog.

## Diagnosis

Reproduced with the real-window media harness (`run-media.py`, private display)
and the reported recording swapped in locally (never committed), with WebKit
compositing enabled as on a desktop session. A temporary log of media
requests (offsets and durations only) was used and removed afterwards.

- **Size (1).** With `preload="none"` a video has no intrinsic size, so the
  element showed at the 300×150 default.
- **Stalls (2, 3).** In dev builds one 64 KiB range read took ~10 ms
  (SHA-256 of the ciphertext ~1 ms, XChaCha20-Poly1305 open + zeroize ~5–7 ms,
  plus a second chunk for unaligned starts), so the listener delivered ~2.5 MB/s.
  WebKit's media source throttles by cancelling and re-requesting ranges;
  against the slow source it made 33–47 requests for one file, and playback
  stuck at ~0.1 s with readyState 2 in every compositing run. The same file
  from a plain local range server played every time with 3–5 requests. Release
  builds (~0.3 ms per range) played every time. The broken-video symbol was not
  reproduced; no connection refusal or media error occurred in any run.
- **Metadata preload.** `preload="metadata"` showed the first frame, but
  playback then stalled at ~6 s in 5 of 8 runs **with the plain server too**:
  a WebKitGTK behaviour, not this transport. `preload="auto"` played 6/6 on the
  plain server.
- **Verification (4).** Already streaming: `save_attachment` authenticates
  64 KiB chunks into a fixed prefix collector; no whole attachment is held.
  Its cost was the same per-chunk crypto cost (≈1.2 s for 10 MiB in dev
  builds before this plan, tens of ms after).

## Scope

- Dev-profile optimization of `qv-crypto`, `chacha20`, `poly1305`,
  `chacha20poly1305`, `sha2`, `zeroize` (dev/test builds only; release
  unchanged, no dependency or `Cargo.lock` change).
- Preview streaming reads stop at 64 KiB chunk boundaries (one chunk per read).
- Video player fills the viewer stage, letterboxed.
- Opened video previews use `preload="auto"`; audio stays `none`.
- Spec text for the preload change; media validation flow asserts it.
- Added after the security finding (owner chose "A", then the app's own
  player, 2026-11-14):
  - Linux plays verified media from renderer memory (`assets/player.js`): an
    MP4 is rewritten losslessly into Media Source Extensions fragments from
    its own sample tables (init segment plus ~1 s fragments, video cut at
    sync samples, each track starting at 0); other files, and any MP4 the
    converter or engine refuses, play as a blob. Page copies are overwritten
    after appending and dropped on close/lock.
  - The listener grants CORS to the app's own Origin only; the Linux CSP adds
    `blob:` to media-src and the listener to connect-src.
  - The app's own controls on every platform (Rust renders them, `player.js`
    wires them): play/pause, seek, time, mute, volume, keys Space/K, M, F;
    video on desktop has the full-screen button (window full screen) in the
    bar, double click toggles it, and the controls hide after 2.5 s of a
    still pointer while playing.
  - The synthetic `preview.mp4` fixture is regenerated with a keyframe every
    second (`-g 12`).

Out of scope: the transport itself (Open question; owner kept the listener), caching keys or plaintext
between range reads (0003 §6.4 forbids a detached key), store API changes.

## Spec basis

0003 §6.4 required `preload none`. The owner asked for the first frame before
play (item 5), which is the go-ahead for amending it: an opened video may
preload; audio still does not. Opening the preview is the explicit action, the
content is already hash-verified and at most 10 MiB (§6.3), and images already
decode on open. 0009 §1.8 mirrors the rule.

## Open question (owner)

Whether to keep the loopback listener for media. Facts: the listener is the
Dioxus desktop edit transport — the whole UI (DOM edits and events) runs over a
WebSocket on `127.0.0.1` with a random port and secret key, on every platform.
Previews reuse it with a separate random per-preview capability (plan 0050).
Removing media from it alone would not remove the listener. Alternatives and
trade-offs are summarized in the owner report of 2026-11-14; nothing changes
until the owner decides.

Update 2026-11-14: the WebSocket listener is upstream Dioxus desktop (its
transport since wry IPC bugs on Android), not QuickVault's. Moving media to the
in-app custom protocol (with MSE playback, no range requests) would remove only
the preview routes, their HTTP parsing and the CORS grant. Owner: not much
benefit while the listener stays; media keeps the listener for now.

## Steps

1. Dev-profile overrides in the workspace `Cargo.toml`. — done
2. Chunk-aligned reads in `preview::Source::stream`; unit test of an unaligned
   range to the end. — done
3. Viewer CSS: `.v-video` stretches, `video` fills with `object-fit: contain`.
   — done
4. `preload: "auto"` for video; 0003 §6.4 / 0009 §1.8 amended. — done
5. `media.js`: video preloads `auto`, has its first frame (readyState ≥ 2,
   128 px wide) while paused at 0 and wider than 300 px, then plays. — done
6. Gates; native media flow with and without compositing. — done
7. Memory playback on Linux: blob, then MSE with the MP4 converter; CORS and
   CSP; spec 0003 §6.4 / 0009 §1.8 amended. — done
8. The app's own player controls and in-bar full screen. — done
9. Media flow drives the controls, asserts MSE for the MP4, replay after the
   end and the full-screen toggle; fixture regenerated. — done
10. Required gates on the final tree. — pending (owner asked to be consulted
    before heavy runs while active)
11. Fresh-context review (preview streaming and renderer plaintext handling
    are secret-handling code). — pending

## Test plan

- `cargo test -p qv-app` (preview unit tests, including the new range case).
- Native media flow (`run-media.py --private-runtime`), compositing off and on.
- Required gates on the final tree.

## Execution log

- 2026-11-14: Diagnosis above. Per-range timing in dev builds: ~10 ms before,
  ~0.4 ms after step 1. Reported recording, dev build, compositing on:
  before, every run stalled; with steps 1–2 and preload none, 11/12 played;
  with `preload="metadata"` 5/8 stalled at ~6 s (plain server 5/8 too); with
  `preload="auto"` 7/8 played (the remaining run stuck at 0.07 s after WebKit
  issued two concurrent requests for the same range; plain server 6/6). The
  first frame was ready before play (readyState 4) at 1072×630 in all runs.
  The residual stall is recorded, not fixed. Temporary probes and the swapped
  fixture were restored (fixture SHA-256 unchanged).
- 2026-11-14: Owner report: after a video ends, play again sticks on a frame;
  dragging the time back first works. `app.js` now pauses and rewinds viewer
  media on `ended`, so play again starts from a plain paused seek. Syntax
  checked only; the owner's retry and a media-flow replay check are pending.
- 2026-11-14: Owner report: native fullscreen shows black while the controls
  and time keep advancing (playback alive, source not revoked). The viewer's
  rounded clip and shadow on the video are removed under `:fullscreen` /
  `:-webkit-full-screen`. Hypothesis, not verified; awaits the owner's retry.
- 2026-11-14: Fullscreen button moved to window full screen (`viewer-full`);
  WebKitGTK native full screen disabled (`set_enable_fullscreen(false)`).
  The `ended` rewind was removed (owner: a hack, and replay still failed).
- 2026-11-14: **Security finding (Linux, pre-existing since plan 0050).**
  During playback the WebKit web process held an unlinked temp file
  `/var/tmp/WebKit-Media-XXXXXX`. WebKitGTK 2.48 source:
  `MediaPlayerPrivateGStreamer::play()` forces preload Auto. Then
  `updateDownloadBufferingFlag()` enables GStreamer on-disk download
  buffering for any non-live, non-blob, non-looping media.
  `configureDownloadBuffer()` always puts it under `/var/tmp` on GTK, with no
  setting or environment override (`isMediaDiskCacheDisabled()` is WPE-only).
  So decrypted preview audio/video reaches the disk, against 0003 §6.4 ("no
  temporary or cache file"); the incognito (ephemeral) context does not
  prevent it. Replays after the end run from that buffer without any request
  to the listener, and WebKit calls this buffering "deadlock-prone" (it
  disables it for looping media), which matches the replay stall. Fix
  pending owner direction; see the owner report of 2026-11-14.
- 2026-11-14: Owner chose option A (play verified media from memory).
  Implemented on Linux:
  - the viewer renders `data-qv-src`; `app.js` fetches it into a Blob, plays it
    from a `blob:` URL, shows the controls once loaded, and revokes on
    stop/close/lock;
  - the listener echoes the app's own accepted Origin as the only CORS grant;
  - the Linux CSP adds `blob:` to media-src and the listener to connect-src.
  Results:
  - regular media flow (compositing on) passes;
  - no `/var/tmp/WebKit-Media-*` file was open during it, only memfd buffers.
  Probes with the reported recording (WebKitGTK 2.48.7):
  - a blob of a file with its index (`moov`) at the end never advances
    (time 0, readyState 4);
  - an index-first copy (lossless ffmpeg remux, local only) plays once, but
    play after end sticks at 0;
  - so the replay stall is a WebKit bug with HTTP and blob sources alike.
  Process errors during these runs, both fixed:
  - a probe-restore script reverted the CORS change, so several runs had no
    CORS;
  - a loader rewrite deleted `revokeHiddenPreview`/`clearRevealed`/
    `currentFormEpoch`; they were restored from HEAD and the loader returned
    to the passing version.
  Open: index-at-end MP4s on Linux, replay after end, spec text, full gates,
  review.
- 2026-11-14: Owner: no workarounds for engine faults; asked about a better
  player. MSE chosen with the app's own controls (owner: "lets do the change
  u suggested"). Probe with the reported recording, FFmpeg-fragmented, fed to
  a MediaSource: played to the end, play after end worked twice, seeks
  worked, no `/var/tmp` media file. Implemented `player.js` (converter, MSE
  and blob playback, controls). Converter checked outside WebKit (Node) on the
  fixture and the reported recording: correct `moof`/`trun` timing, video
  starting at 0.
- 2026-11-14: Replay findings (WebKitGTK 2.48.7, MSE, one run each):
  - an FFmpeg-fragmented fixture (video starting at 0.17 s) never plays: a
    gap at the start stalls WebKit, hence each track starts at 0;
  - applying edit lists (audio priming 46 ms) left an audio gap; replaced by
    starting every track at its first presented sample;
  - fixture variants: replay fails ("No valid frames decoded before end of
    stream") only for B-frames with a single keyframe (original fixture,
    video only); keyframe every second, no B-frames, and audio only replay;
  - seeking to 0 before play, 1 s fragments, or `timestampOffset` instead of
    shifted composition offsets do not help (the offset variant plays worse).
  Recorded as an engine limit; the fixture now has a keyframe per second.
  Short OBS clips (under ~8 s at OBS's default keyframe interval) can hit it.
- 2026-11-14: Results on the final tree:
  - native media flow passes with compositing on (no `/var/tmp` media file
    during it) and off; `qv-app` clippy with `validation` passed;
  - screenshots (local, the reported recording): first frame and 0:00 / 0:12
    before play, playing with the bar, idle with the bar hidden, window full
    screen with the bar, audio card with the same controls.
  Probes, the swapped fixture and the probe logging were restored.
- 2026-11-14: **Owner retry (release build, desktop, the reported recording):
  not fixed; earlier claims corrected.**
  - Opening the video: nothing plays for a long time and the whole UI
    freezes for a while, then playback starts. Dragging the seek bar makes the
    freeze worse.
  - Play after the end: the picture stays on one frame for a long stretch,
    then the end plays normally.
  - The recording has only two keyframes (0.00 s and 8.33 s; 383 video
    samples, B-frames; AAC audio), not one every 1–2 s as stated above. The
    frozen stretch matches "no picture until the next keyframe".
  - The media flow's replay check (`!paused`, `0.3 < t < 3`) checks the
    clock only, not shown frames, so the "replay works" results above are
    unverified. The `-g 12` fixture would hide the fault (keyframe every
    second).
  - Ruled out:
    - the converter's run time (~5 ms on the recording, Node);
    - hardware decoders: the freeze is the same with `vah264dec`, `nvh264dec`
      and `v4l2slh264dec` ranked NONE.
  - Earlier screenshot runs with compositing on lagged badly with this
    recording; that lag was put down to the screenshot tool, but is probably
    this fault.
  - Hypothesis, not tested: the converter shifts composition offsets so
    each track starts at 0. With B-frames this gives some samples a
    presentation time before their decode time, which decoders may handle
    badly after a flush or seek.
  - Next: measure shown frames (`getVideoPlaybackQuality()` or
    `requestVideoFrameCallback`) and main-thread stalls with compositing on.
    Compare unshifted offsets (pts ≥ dts) and FFmpeg's fragmenting. The
    owner is getting a second opinion first.
