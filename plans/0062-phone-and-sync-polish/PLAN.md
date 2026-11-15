# Plan 0062 — Phone and sync polish

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — focused checks pass; phone check and fresh-context review pending | 2026-11-15 | 2026-11-15 (owner QA report; design delegated by the owner, standing 2026-11-02) | 0009 §1.8 (amended) |

## Intent

Owner QA on the phone and desktop found six polish issues:

1. Android Back with the sync code window open does not close it; it asks
   for another Back to close the app.
2. While the other device syncs, the device showing the code keeps showing
   it with its countdown: no sign that a sync is under way.
3. On the phone, the "1 item needs your choice" receipt stays after
   "Resolve now", taking much of the screen.
4. The on-screen keyboard pushes the phone tab bar up above it.
5. The per-snapshot delete button is gray.
6. The "Copied · clears in" bar shrinks in steps, not smoothly.

## Findings

1. `Controller::shortcut` returns early while an operation runs
   (`working`), and the code window runs as an operation for its whole
   life. Escape (Android Back) is therefore unhandled and reaches
   `back_at_root`.
2. The core reports admission (`ApproverUi::admitted`), and the app then
   drops the code and QR. But the joining device tries the QR's endpoints
   one at a time (0013 §3.3), waiting up to 10 s for each
   (`LanTimeouts::connect`). Local address discovery lists every directly
   attached IPv4 network first, in routing-table order: on a desktop with
   container bridges (`docker0`, `br-…`, down with no carrier) those
   unreachable addresses come before the LAN address, so the joiner spends
   up to 10 s on each before it connects. The offering side correctly shows
   the code meanwhile. After admission the window kept its countdown and
   the title "Connecting with another device".
3. "Resolve now" navigates, which bumps the screen epoch, then calls
   `lan_dismiss` with the old epoch, so the dismissal is ignored as stale.
4. The Android shell pads the content by the keyboard inset, so the page
   shrinks and its bottom tab bar rides on the keyboard.
5. The row button is a plain `icon-btn`; destructive icons elsewhere use
   `icon-btn danger`.
6. The bar's width class changes every 2 s (`w{secs.div_ceil(2)}`).

## Design (delegated by the owner, standing 2026-11-02)

1. Escape and Back handle the LAN operation before any other shortcut or
   the `working` check: a running window or sync is cancelled (as its
   Cancel button does), a finished receipt is closed. On every platform,
   locked screen included (`Controller::lan_escape`).
2. Discovery skips a directly attached network whose interface reports no
   carrier (`/sys/class/net/<iface>/carrier` is `0`), and lists physical
   interfaces (with a `device` link) before virtual ones, keeping table
   order within each group. Unreadable link state keeps the network
   (Android, other systems). Interface names from the bounded route table
   are used in a path only when they are a plain name (1–15 bytes of
   `[A-Za-z0-9._@-]`, not `.` or `..`). The fixed probes are unchanged.
   Advertised endpoints stay untrusted hints (0013 §1.4); dialing stays
   sequential (0013 §3.3). After admission the window shows "Connected to
   another device" with the sync animation and no countdown (the session is
   still bounded by the window's deadline).
3. "Resolve now" closes the receipt first, then opens the conflicts.
4. On a touch screen, while an input or text area has focus and the
   window is at most 80 % of its tallest height at that width, the root
   gets `data-ime` and CSS hides the tab bar (`app.js`). The save bar stays.
5. The row delete button uses `icon-btn danger` (red icon, soft red hover).
   The confirmation dialog's button already uses `btn danger`.
6. The bar's width follows the seconds left (`width` in percent of 20 s,
   ending at 0) with its existing 1 s linear transition, so it drains
   continuously.

Rejected: parallel dialing (changes 0013 §3.3's one-at-a-time rule, which
limits a wrong responder to one hello); hiding all bridge interfaces (a LAN
bridge `br0` on a VM host is the real LAN); shorter connect timeouts (slow
Wi-Fi joins); a native IME callback into the page (needs a script injection
path the hardened webview does not have).

## Spec basis

0009 §1.8 amended (Android shell, Back closes the sync window; the tab bar
hides while the keyboard is open). 0013 unchanged.

## Steps

1. Spec amendment, plan index, `CHANGELOG.md`.
2. `lan.rs`: `lan_escape`, `LanView::admitted`; `ui/mod.rs` key handler.
3. `pages.rs`: post-admission title and countdown; resolve order; snapshot
   row danger button. `shell.rs`: smooth clipboard bar.
4. `qv-platform/src/net.rs`: link-aware ordering, with unit tests.
5. `app.js`/`app.css`: keyboard tab-bar hiding.
6. Focused checks; APK build and install for the owner's phone check.

## Test plan

- Unit: `attached_probes` skips no-carrier interfaces, orders physical
  before virtual, keeps unreadable ones, refuses unsafe interface names;
  `lan_escape` cancels a running window and closes a receipt.
- Owner, phone and desktop: each of the six reports.

## Execution log

- 2026-11-15: findings above; steps 1–5 implemented.
- 2026-11-15: owner QA follow-up on desktop. (7) The receipt and toasts were
  centered on the window, not the main pane: offset by the sidebar (and
  list) per layout, clamped to the window. (8) A conflict whose versions
  had the same visible content showed one column and one "Keep it"; the
  owner could not tell the versions apart. Unchanged saves on two devices
  merge without conflict (tested), but edits that end equal conflict by
  their save times. The comparison ignored field names, types, hidden
  settings and order, so its "nothing differs" banner could overstate:
  those are now compared, and every version has its own Keep button. Tests:
  `identical_conflict_versions_compare_and_resolve` (keeps the second
  version), `conflict_compare_shows_a_renamed_field`. The owner's later Keep and
  confirmation resolved it; why the first attempt did not was not
  established (the controller path passes the test above).
