# Plan 0068 — Phone UI polish and a stuck scan panel

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — gates pass; phone check and fresh-context review pending | 2026-11-17 | 2026-11-17 (owner QA report; design delegated by the owner, standing 2026-11-02) | 0009 §1.7, §1.8 |

## Intent

Owner QA on the phone found:

1. Settings, quick unlock: with fingerprint unlock on, the main button reads
   "Turn on fingerprint unlock again"; the screen-lock button runs past the
   screen edge; "Turn off quick unlock" looks like plain text.
2. The vault switcher sheet: "Open a vault file…" sits alone in a
   two-column grid with empty space beside it.
3. Creating a vault on the phone: the wizard is wider than the screen and
   scrolls sideways.
4. The unlock screen scrolls sideways although its content fits.
5. Once, after a successful scan and a fast sync, the page kept the "Scan a
   QR code" panel with the camera closed, and "Cancel scan" did nothing.

The owner also asked for any other UI polish found along the way.

## Findings

1. `QuickSettings` offers re-enrolling the active policy ("again"), which
   only restarts the seven-day period; a password unlock after expiry
   enrolls again anyway (0009 §1.8). `.btn` is `white-space:nowrap`, so the
   long label cannot wrap. "Turn off" is `btn subtle` (no fill, no border).
2. `.vmore` is a fixed two-column grid for three actions.
3. The name step's "Saves in …" banner carries `hidden` on Android, but
   `.banner{display:flex}` overrides the user-agent `[hidden]` rule, so the
   private app path shows; that unbreakable path widens the wizard's grid
   track (`.locked-body` uses an `auto` column, which grows to the content's
   minimum width).
4. `.locked::before` is a 900 px glow inside the scrolling `.locked`, so it
   adds horizontal (and on short screens vertical) scroll range.
5. Not reproduced. Two paths can leave `State::scan` set after its thread
   ends: the thread clears it only while the form epoch is unchanged, and
   `cancel_qr` only cancels the token, so a scan left behind is never
   cleared and its Cancel does nothing. Separately, the UI poll records a
   new snapshot as seen even when its redraw signal write fails, so that
   change is not drawn until the next one; during a LAN run or receipt the
   snapshot is otherwise still (no idle countdown), so the screen can stay
   stale until a tap.

Also found: the Settings row says "Fingerprint unlock is on" for the
screen-lock policy, and on Android Settings shows the app's private
directory as the vault place and as "Folder for new vaults" (the locked
screen already hides it, plan 0045).

## Design (delegated by the owner, standing 2026-11-02)

1. Quick unlock card by state:
   - Off: hint, master password, then stacked "Turn on fingerprint unlock"
     (primary) and "Use your screen lock instead (lower protection)…".
   - Fingerprint on: the "on" banner, the hint, a "Turn off fingerprint
     unlock" button (a normal bordered button), then a separate "Lower
     protection option" block with the master password and "Use your screen
     lock instead…".
   - Screen lock on: the banner, the hint, "Turn off screen-lock unlock",
     then the master password and "Switch to fingerprint unlock" (primary).
   No re-enrollment of the active policy. Actions and confirmations are
   unchanged (`EnrollQuick`, `DisableQuick`, the reduced-protection prompt).
2. On phones, buttons may wrap (`white-space:normal`, `max-width:100%`, a
   minimum height instead of a fixed one).
3. Vault sheet: one action per row, like the vault cards above them.
4. The Android name step renders no place banner; a global
   `.qv [hidden]{display:none!important}` makes `hidden` reliable; the
   locked grid column is `minmax(0,1fr)` and banner text wraps anywhere.
5. The locked-screen glow becomes the `.locked` background (same size,
   centre and colours), adding no scroll range.
6. Scan: each scan has an id. The scan thread clears `State::scan` when it
   is still its own scan, whatever the epoch (errors still report only to
   the current form). `cancel_qr` clears it at once and redraws; the thread
   then finds no scan and starts nothing. A new scan still waits for the
   worker (`working`) to be released. The UI poll keeps a snapshot unseen
   until its redraw write succeeds.
7. Settings: the quick-unlock row names the active policy; on Android the
   vault row shows only the file name and the folder row is not shown.

Rejected: auto-hiding the scan panel on a timer (hides a real running scan);
a separate renewal button (expiry already re-offers fingerprint unlock).

## Spec basis

0009 §1.7 (scan is cancellable, bounded) and §1.8 (owner-delegated UI
details; Settings and locked screen on Android show no private path). No
normative change; formats, protocols and secret handling are untouched.

## Steps

1. [x] `app.css`: `[hidden]`, locked glow and grid, phone button wrapping,
   vault sheet list.
2. [x] `ui/mod.rs`: no Android place banner; `ui/pages.rs`: quick unlock
   card, Settings rows.
3. [x] `controller.rs`, `qr.rs`: scan identity and immediate cancel, with a
   unit test; `ui/mod.rs`: poll retry.
4. [x] Headless Chromium checks of the phone layouts at 360 and 390 px
   (static copies of the rendered markup with `app.css`, synthetic text).
5. [x] Gates; CHANGELOG, plans index.

## Test plan

- Unit: a scan left behind after an epoch change is cleared by its own
  thread's completion path; `cancel_qr` clears the panel at once and a
  late payload is refused; a newer scan is not cleared by an older one.
- Layout: no horizontal overflow (`scrollWidth <= clientWidth`) on the
  unlock screen, create wizard, quick-unlock card and vault sheet at 360 px.
- Gates: `sh scripts/check-spdx.sh`, `cargo fmt --check`,
  `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`,
  plus qv-app `--features desktop` and `--features validation` clippy.
- Owner, phone: each of the five reports.

## Risks

- Phone-only CSS could change desktop layout: all new rules except
  `[hidden]` and the glow are inside the 640 px container query.

## Revisions

- 2026-11-17, owner phone QA, second batch (design delegated):
  1. The weak-password banner's two buttons had different widths, so
     their edges did not line up: on phones, actions inside a banner stack
     at one width.
  2. The hide button showed only the 30 s countdown ring: it now shows the
     crossed eye inside the ring.
  3. The owner asked whether groups nest. They do ("New subgroup" in a
     group's menu, 0009 §1.8); the move dialog's "No groups inside this
     one." now names the empty level ("No groups yet…" at the top, "“X”
     has no subgroups…" inside a group).
  4. Notice toasts had no timer: a notice now clears itself after
     `notice_for` (about 12 characters a second, 4–12 s; at least 8 s with
     Undo, which goes with it), timed from when the idle UI poll first sees
     it; errors still stay until dismissed. The close button remains.

## Execution log

- 2026-11-17: findings and design above.
- 2026-11-17: steps 1–3 implemented. Revision: the vault sheet first kept
  its grid with the odd last action spanning both columns; the headless
  check showed "Join a device" wrapping in its half-width tile, so the three
  actions became one per row. In `scan_done` an error or a start also needs
  the scan to still be the running one (an older scan's end reports
  nothing). `apply_qr` takes the scan id instead of a flag, so a code counts
  only for that same running scan.
- 2026-11-17: step 4. Static copies of the rendered markup (synthetic
  text) with the old and new `app.css`, in headless Chromium at 360 px
  (`.qv` inside a transformed 360×740 box so fixed layers and container
  queries use that width), measuring scroll width and button bounds. Old
  CSS: the unlock screen scrolled 630/360 px; the Android create step
  (with the `hidden` banner showing) and a long desktop place pushed
  Continue/Change past the edge; the quick-unlock buttons ran to 526 px.
  New CSS: no horizontal overflow or clipped button on any of them; light
  and dark screenshots of the vault sheet and quick-unlock card checked.
- 2026-11-17: step 5, final tree: `sh scripts/check-spdx.sh`, `cargo fmt --check`,
  `sh scripts/check-matrix.sh`, `cargo clippy --all-targets --locked -- -D warnings`,
  qv-app clippy with `--features desktop` and `--features validation` (only
  the existing vendored dioxus-desktop `show_toast` warning), all exit 0;
  `cargo test --locked`: 1,092 passed, 0 failed, 8 ignored;
  `RUST_TEST_THREADS=1 cargo test --locked -p qv-app --features validation`:
  114 tests and 3 real-engine flows pass. Android arm64 Clippy was not run
  (no Android standard library for the pinned toolchain here); the changed
  Android paths are `cfg!` branches compiled on Linux, and the
  `#[cfg(target_os = "android")]` call in `cancel_qr` is unchanged. No APK
  was built.
- Found, not fixed: the stuck scan panel was not reproduced, so its exact
  trigger is unknown; the repairs above close every path found that leaves
  the panel up or the screen stale. If it recurs, note what was on screen
  just before (receipt, error, a second tap on Scan).
- 2026-11-17: Debug APK built with `crates/qv-app/scripts/build-android.sh`
  (SHA-256 c7c3d1ab…ed3af, QR camera on). The phone's release-signed app
  refused the update; with the owner's OK it was uninstalled (its data
  discarded) and the debug APK installed and launched. Owner phone check
  pending.
- 2026-11-17: second batch gates: SPDX, fmt, matrix, workspace clippy,
  qv-app `desktop` and `validation` clippy and `cargo test --locked` all
  exit 0. The qv-app `--features validation` test run was stopped to build
  the APK for the owner and not rerun (owner: CI runs it on push). Debug
  APK (SHA-256 80c44eca…d84c64) installed over the earlier debug build
  (data kept) and launched.
