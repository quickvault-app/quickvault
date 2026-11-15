# Plan 0061 — Audio preview bars follow the sound

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — Linux probe clean; focused checks pass; full gates, Android phone check and fresh-context review pending; native levels proposed | 2026-11-15 | 2026-11-15 (owner request; design delegated by the owner, standing 2026-11-02) | 0003 §6.4 (amended), 0009 §1.8 (amended) |

## Intent

The owner liked the audio preview card and asked for its moving bars to follow
the audio instead of a fixed CSS loop. After trying the first version on
Linux they reported: the bars looked silent while audio played, pausing and
unpausing lagged, and the bars should not move before Play.

## Findings

- The 24 bars (`ui/items.rs`, `.v-eq`) ran a CSS `@keyframes eq` loop with
  staggered delays, from the moment the card opened, and never read the audio.
- The engines expose levels through Web Audio: a `MediaElementAudioSourceNode`
  into an `AnalyserNode`. Once routed, the element is heard only through the
  context, so a refused, suspended or tainted route plays **silence**, not
  just still bars.
- **Origins.** Linux plays from renderer memory (`blob:` URLs, MSE or a whole
  blob, 0003 §6.4): same origin. Android and Windows play the loopback
  listener's URL, a different origin. Without CORS, Chromium routes a
  cross-origin element as zeros. The listener already echoes CORS for the
  app's own origin; on Android that origin (`https://dioxus.index.html`) is
  accepted, so `crossorigin="anonymous"` on the Android audio element is
  enough. The listener's non-Android allow-list (`dioxus://index.html`,
  `null`) does not cover the Windows page origin, so a CORS request there
  would be refused. Windows keeps the CSS loop; widening the vendored
  listener's origin list is a trust-boundary change left out of this plan.
- **Gesture.** An `AudioContext` created outside a user gesture stays
  suspended in WebKitGTK (`resume()` never settles) and an element routed
  into it refuses `play()` with `NotAllowedError`; one created and resumed
  synchronously inside the click runs.
- **WebKitGTK routing is not clean (engine limit).** WebKitGTK 2.48 probe:
  private display and PipeWire null sink, the real `player.js` loading paths
  (`loadBlob`, MP3 blob and M4A through MSE) behind a local range server, and
  a 20 s chirp (200 Hz + 100 Hz/s) recorded from the sink monitor, so the
  pitch gives the position actually heard. Without routing, the sound is
  continuous and each pause is a gap of its own length. With routing:
  - routed once playing (the first version): about **1 s of sound is
    skipped** when routing starts, and each unpause adds about **0.8 s of
    silence** before sound returns; the analyser meanwhile reads near zero
    (the owner's "looks muted but it's playing" and the pause lag);
  - routed before the first play: mostly silence;
  - suspending and resuming the context leaves an MSE source silent until
    the next seek.
  The earlier short mono probes (one-shot MSE append, whole WAV) hid this
  because they sampled levels and `currentTime`, not the audible output.

## Design (delegated by the owner, standing 2026-11-02)

- The bars rest until playback starts (`data-playing` on `.v-eq`, set from
  the player's state); the CSS loop runs only while playing. On pause or end
  they glide down over 0.45 s from their current heights (frozen inline,
  then released under `data-settling`): a removed animation or dropped
  inline height would otherwise snap them to rest (owner request).
- Real levels only where routing does not change what is heard: an element
  that requested its source with CORS (Android, Chromium). Linux (WebKitGTK)
  and Windows keep the loop while playing. No WebKit workaround (owner:
  no hacks over engine bugs).
- One page-wide `AudioContext`, created and resumed synchronously in the play
  button's click (the keyboard shortcuts click the same button). The element
  is routed on `playing`, or when the click's resume settles while it plays,
  only when the context is `running`. Any failure leaves playback untouched
  and the bars on their loop. The analyser is connected to the destination
  before the source node is made, so a throwing step never leaves the
  element routed to nothing.
- One analyser per element (an element can be routed only once), kept in a
  `WeakMap`. `fftSize` 2048; 24 log-spaced bands from 50 Hz to 14 kHz, each
  the peak of its bins, drawn by the player's existing animation frame.
  Paused bars rest; the bins array is zeroed on every pause.
- The context is never suspended. When the last routed element leaves the
  document (the preview closed), the context is closed, releasing the audio
  device; the next Play click creates a new one.
- `prefers-reduced-motion: reduce` keeps the bars static (no routing).
- Android audio gets `crossorigin="anonymous"`; Linux and Windows markup is
  unchanged.

**Secrets.** On Android the analyser reads decoded audio that the engine
already holds, as 1024 coarse bytes per frame in a page array zeroed on
pause. Engine analyser buffers keep 0008's best-effort limits, like decoder
copies. No new plaintext copies, files, fetches or origins.

Rejected: routing on WebKitGTK (audible skip and added silence, above);
routing before the first play (silence); suspending on pause (MSE silence);
a hidden second element routed only for analysis (doubles decoding and range
reads, and is a workaround over the engine bug); decoding the file again with
`decodeAudioData` (a second whole plaintext copy); routing cross-origin
sources without CORS (silent playback); a Windows-only listener origin change
in this plan (vendored trust boundary).

## Proposed follow-up: native levels (not started)

Real bars on every platform without engine routing: when an audio preview
opens, decode the verified attachment in Rust through the existing protected
range reads, compute a compact level envelope (24 bands at about 30 frames
per second, roughly 170 KB for 4 minutes) and draw it against `currentTime`.
Playback stays untouched on every engine. Costs: a decoder dependency
(Symphonia is the candidate; licence against `deny.toml`, `cargo deny`/`vet`),
another parser of untrusted plaintext (safe Rust, bounded), CPU while
opening long files (progressive, cancellable), and a derived level array in
page memory. It handles plaintext attachments, so it needs its own
architect → developer → reviewer plan.

## Spec basis

0003 §6.4 and 0009 §1.8 amended: the bars rest until playback starts; the
audio preview may show levels from the engine's analyser where routing does
not change what is heard; Android audio uses a CORS request granted to the
app's own origin by the existing listener rule.

## Steps

1. Spec amendments (0003 §6.4, 0009 §1.8, Changelogs), plan index,
   `CHANGELOG.md`.
2. `player.js`: context, routing (CORS sources only), band drawing, cleanup,
   play-state attribute.
3. `app.css`: resting bars, loop only while playing. `ui/items.rs`: Android
   `crossorigin`.
4. Focused checks: SPDX, fmt, qv-app desktop and validation clippy; the
   WebKitGTK probe re-run against the final code.

## Test plan

- WebKitGTK probe (scratch, not committed; real `player.js` loading paths,
  play via a UI-process click, sink monitor recorded): the heard position is
  continuous and pause gaps equal the pauses, as without the analyser; the
  bars have no animation before Play and loop while playing.
- Existing media harness behaviour is unaffected: no element is routed on
  Linux.
- Owner/phone: Android audio preview plays with continuous sound, moving
  bars that follow it, and clean pause/unpause; Linux bars rest before Play
  and loop while playing; Windows the same as Linux.

## Execution log

- 2026-11-15: first version routed blob and CORS sources; short probes passed.
- 2026-11-15: owner report (bars silent while playing, pause lag, bars moving
  before Play). The recorded-output probe above showed the WebKitGTK skip and
  added silence; routing was limited to CORS sources (Android), bars now rest
  until playback. Re-run: Linux output continuous, identical to the
  analyser-free baseline; no animation before Play.
- 2026-11-15: owner request: bars snapped down on pause. Added the settle
  glide; WebKitGTK probe: the average bar height eases from its playing value
  to rest over about 0.45 s instead of dropping in one frame.
