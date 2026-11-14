# Plan 0057 — Video preview: playback stalls, player size, first frame

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — seek after end verified; fresh-context review pending | 2026-11-14 | 2026-11-14 (owner bug report and request; design delegated by the owner, standing 2026-11-02) | 0003 §6.4 (amended), 0009 §1.8 (amended) |

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
- Opened video previews use `preload="auto"`; Linux memory audio loads on open
  too, and other platforms use audio metadata preload (step 7's amendment).
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

0003 §6.4 originally required `preload none`. The owner asked for the first frame
before play (item 5), followed by memory playback and the app's own controls.
The amendments allow opened media to load without autoplay. Opening the preview is the explicit action, the
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
10. Replay repair and meaningful regression coverage (revision below). — done
11. Required gates on the final tree. — done
12. Seek after the end (revision below). — done
13. Required gates on the updated tree and release rebuild. — done
14. Fresh-context review (preview streaming and renderer plaintext handling
    are secret-handling code). — pending

## Revisions

- 2026-11-14 (owner continuation, seek after end): the owner confirms normal
  playback/replay and reports a frozen picture when seeking backward with the
  slider after EOS. Continue on the owner's staged tree, preserving the index.
  This extends the approved memory source lifecycle; no new dependency,
  storage/wire format, detached key or owner decision. The earlier ordinary
  seek path remains for a source that has not ended.

### Step 12 — Seek from an exhausted source

- [x] **Do:** Amend 0003 §6.4 / 0009 §1.8 to include a backward seek after EOS:
  rebuild the Linux memory source from the live capability, restore the chosen
  playback position and keep the paused state. Choosing a position alone must
  not start playback. Extend the private helper to
  `loadBlob(media, startPlaying = false, startAt = 0)`; after attaching the
  fresh source, apply the requested default playback position before play.
  The slider change handler captures/clamps a finite target in `[0, duration]`
  before any reset; a backward target on an ended memory source calls the
  loader with that target. Selecting the end itself leaves EOS untouched;
  other seeks preserve the existing source/state.
  Keep identity/connectivity/abort checks before attaching or seeking, and
  never cache media bytes for this action. If WebKit needs metadata before
  applying the position, any added listener must be abortable and removed
  before a stale source can act.
- **Files:** `crates/qv-app/assets/player.js`, `tests/player.cjs`,
  `src/bin/media.js`, `tests/fixtures/README.md`, `crates/qv-app/README.md`,
  specs 0003/0009 and their index, `CHANGELOG.md`, this plan/index.
- **Done when:** After natural EOS, slider seek to 1 s shows a frame near 1 s
  while paused; Play advances frames near that position before the 8.33 s
  keyframe. Repeat after EOS with target 0; replay and ordinary paused/playing
  seeks still work. A stale/aborted load cannot restore the requested position
  or play after close/lock. Preserve the existing replay/drag/reset checks.
- **Verify:** `node --test crates/qv-app/tests/player.cjs`; native media flow
  serially with/without compositing and `--private-runtime --check-media-files`.
  Establish failure on the current player before repair; probe the owner's
  original recording in memory. Run the required gates on the final tree and
  rebuild the desktop release. Stop to revise the design if restoring the
  selected position on a fresh source does not recover moving frames.

- 2026-11-14 (owner continuation, replay repair): the owner identified PipeWire
  as part of the startup fault; normal playback now works, but replay freezes
  until the second keyframe. The owner also requests that releasing a player
  control over the backdrop does not close the viewer. Continue this approved
  plan on the clean tree; no new dependency, public Rust API, storage format,
  detached key or transport change. Prior speculative diagnoses remain in the
  execution log as historical evidence, not established causes.

### Step 10 — Replay lifecycle and control gestures

- [x] **Do:** Amend 0003 §6.4 and 0009 §1.8 first: replay on Linux creates a
  fresh renderer memory source from the still-live authenticated capability;
  no app-owned plaintext cache is retained for replay. Keep MSE conversion,
  blob fallback, no autoplay, and close/navigation/hide/lock revocation.
  On an explicit play at end, call `loadBlob(media, true)`; extend that private
  JS function with an optional play flag, and request playback only after the
  fresh source is attached and its ownership/connectivity is rechecked. Ordinary
  pause/resume and seeking keep their existing source. Late fetch completion
  after close/lock must wipe its bytes and never attach or play media.
  Explicitly reset the default playback position after detaching renderer media
  in `stopMedia`. Verify the existing reset assertions, including zero position.
  Guard the viewer click produced by a pointer gesture beginning inside
  `.v-player` and ending outside it: consume that click in capture phase;
  a new pointerdown or pointer cancellation clears the gesture, keyboard
  activation remains usable, and a normal backdrop click still closes.
- **Files:** `crates/qv-app/assets/player.js`, `assets/app.js`, `src/bin/media.js`,
  `src/validation_media.rs`, `tests/fixtures/preview-long-gop.mp4`,
  `src/ui/mod.rs` (bounded synthetic frame-count reports),
  `tests/fixtures/README.md`, focused JS tests under `tests/`,
  `crates/qv-app/README.md`, `plans/0050-complete-audit-remediation/run-media.py`
  (repeatable compositing selection and WebKit media-file sampling),
  specs 0003/0009 and their index, `CHANGELOG.md`, this plan/index.
- **Done when:** A synthetic H.264 clip with B-frames, AAC, index at the end,
  and keyframes at 0 and 8.33 s shows moving frames in the first seconds of
  initial play and two replays. Measure presented frames, not just currentTime.
  Seeking and pause/resume work. Close/lock during replay cannot resurrect the
  source. Drag-release on the backdrop keeps the viewer open; an ordinary
  backdrop click closes it. The owner's recording is read only into memory
  by local probes, never added to the repository or copied to a plaintext file.
- **Verify:** `node --test crates/qv-app/tests/player.cjs`; build the validation
  flow harness and run `run-media.py --private-runtime`, compositing off/on,
  with an assertion against WebKit media temporary files. Run the new native
  regression once against the old player to establish that it fails, then
  against the repair. Probe the owner's recording through the actual repaired
  player and count frames across two replays. Required final gates remain in
  step 11; retain complete outputs in temporary logs. Stop for a design revision
  if source reconstruction does not restore moving frames or persists media.

## Test plan

- `cargo test -p qv-app` (preview unit tests, including the new range case).
- Native media flow (`run-media.py --private-runtime`), compositing off and on.
- Required gates on the final tree.

## Execution log

- 2026-11-14 (seek-after-end final gates, step 13): all required checks pass
  on the updated tree: SPDX, formatting, workspace Clippy with denied warnings,
  and `cargo test --locked` (1,040 passed, 0 failed, 7 ignored). Matrix check,
  validation-feature Clippy and all 11 JS tests also pass. The optimized Linux
  desktop `quickvault` binary was rebuilt successfully after the final asset
  edits. Full outputs are retained in `/tmp/qv-seek-investigation/`. No
  dependencies, Rust APIs or persistent formats changed. The follow-up changes
  remain unstaged, preserving the owner's existing index. Local inspection
  found no further issue in this repair; the fresh-context review is now
  step 14 and remains pending. Test results do not establish security or
  authorize a release.

- 2026-11-14 (seek-after-end repair, step 12): owner-confirmed playback/replay
  works; the new report is a slider seek after EOS. The baseline WebKit probe
  of the owner's recording selected 1 s but presented the 8.4 s frame, followed
  by four one-second intervals with zero new frames while the clock advanced.
  A fresh source with its default start position set before metadata restored
  the frame at 1 s while paused. The actual repaired controls then presented
  about 29 frames/s starting at that selected position. Original media was
  read only into memory, never changed or copied to a plaintext test file.
  Logs: `/tmp/qv-seek-investigation/baseline` and `production`.
- 2026-11-14 (step 12 validation): the native app flows pass serially with and
  without compositing, including paused selections at 1 s and 0 after natural
  EOS, subsequent moving frames, existing replay/drag/close/reset checks, and
  audio lock/revocation. Both descriptor samplers checked WebKit successfully,
  with no observed `WebKit-Media-*` file. Eleven JS tests pass, including
  cancelled/superseded positions, ordinary seeks and the unchanged end-position
  selection. The new unit cases fail against the previous player. The full
  native baseline run encountered a separate stage-72 media initialization
  failure before reaching the new case during a concurrent build; that run is
  not used as seek-specific regression evidence. No assertions, lock timeout
  or frame-progress checks were weakened. Existing staged changes are preserved.
  Logs: `/tmp/qv-seek-investigation/native-after-compositing`,
  `native-after-software`, `unit-before.log` and `unit-after.log`.

- 2026-11-14 (final verification): native media flows pass serially with and
  without compositing, through stages 73–77 (drag-release, natural EOS after
  a seek, replay load, ordinary backdrop close and decoder reset), image
  checks, and audio lock/revocation. Both runs used `--private-runtime
  --check-media-files`, with no observed WebKit download file and no unreadable
  engine descriptor set. Logs: `/tmp/qv-video-investigation/close-stage-compositing`
  and `close-stage-software`. Earlier concurrent media/build runs had intermittent
  extended seek/close timeouts; a detached-media snapshot did not locate the
  failing wait or prove a default-position bug. Added stage markers distinguish
  those waits; the final serial checks passed without disabling assertions or
  the inactivity lock. The retained default position is explicitly cleared and
  covered by a focused test, but is not claimed as the cause of those timeouts.
- 2026-11-14 (gates): `sh scripts/check-spdx.sh`, `cargo fmt --check`,
  `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`
  pass (1,040 passed, 0 failed, 7 ignored). `sh scripts/check-matrix.sh` passes.
  Also pass: validation-feature Clippy; the seven focused JS tests;
  `RUST_TEST_THREADS=1 cargo test --locked -p qv-app --features validation --test engine`
  (3 passed); and the optimized desktop `quickvault` build. Default test inputs
  are unchanged by the final desktop-only cleanup/validation JS edits:
  `cargo test --locked --no-run` confirms no recompilation on the final inputs.
  Logs are under `/tmp/qv-video-investigation/`; no dependency or lockfile change.
  Local diff/lifecycle inspection is complete; step 12 requires a separate
  fresh-context reviewer and remains pending. No security proof or release
  approval is inferred from these checks.

- 2026-11-14 (repair implementation): `loadBlob` accepts an explicit play
  request, and the play control reconstructs the source only at EOS. The
  existing capability/read/cleanup path is reused; no original bytes or muxed
  parts are cached for replay. Late response, replaced request, refused load,
  ordinary pause/resume, direct-source playback and control-gesture cases have
  focused JS tests. The synthetic 12 s B-frame/AAC fixture has keyframes at
  0 and 8.33 s and a trailing index, with documented generation and hash.
  The old compiled player fails the new native composited test at stage 71,
  the first sparse-keyframe replay. The repaired production controls presented
  about 29–30 frames/s on two replays of the owner's recording in memory;
  the recording was not copied to disk, changed or added to the repository.
- 2026-11-14 (verification refinements): WebKit's software renderer advertises
  frame callbacks but does not deliver them, so the native test also samples
  the presented-frame statistic. Its counters reset asynchronously on resume;
  increments are accumulated across that reset. Composited checks use actual
  frame callbacks. No clock-only acceptance was restored. `stopMedia` explicitly
  clears the default playback position with no source/decoder, and the existing
  zero-position reset assertion remains in force. The native runner can select
  compositing and sample WebKit descriptors: its negative control detects an
  intentionally unlinked synthetic `WebKit-Media-*` file. The sampler checks
  the known buffering path, not arbitrary disk writes.

- 2026-11-14 (replay investigation): clean tree. In a private PipeWire session,
  WebKitGTK 2.48.7 reproduced the owner's recording: each replay displayed a
  frame at about 8.4 s, with zero new frames while currentTime advanced through
  2.45–5.43 s. A fresh MediaSource built from a new in-memory read presented
  about 29–30 frames/s over the same interval on two replays. Timestamp-offset
  changes alone did not fix a synthetic reproduction. This establishes the
  failing retained decoder/source lifecycle, not a diagnosis of its internal
  WebKit/GStreamer cause. The owner confirms normal playback after addressing
  PipeWire; system audio configuration is outside this repair.

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
