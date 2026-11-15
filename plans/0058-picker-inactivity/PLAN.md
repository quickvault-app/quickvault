# Plan 0058 — Inactivity lock while a file picker is open

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — APK build, phone check and fresh-context review pending | 2026-11-15 | 2026-11-15 (owner bug report; limit delegated by the owner, standing 2026-11-02) | 0009 §1.8 (amended) |

## Intent

The owner reported: while choosing a file to upload in the system picker, the
vault locked before they found the file, apparently from inactivity.

## Diagnosis

- The picker is another window (Linux portal helper, Windows dialog) or
  another activity (Android SAF). The app receives no input meanwhile, so the
  60-second inactivity lock kept counting.
- On Android the pick deferred only background expiry; 0009 §1.8 said the
  inactivity lock still applies. The lock forgets the pick, so the choice was
  lost as well.

## Design (delegated by the owner, standing 2026-11-02)

- While a system file picker is open, the inactivity limit is 5 minutes
  (`PICKER_IDLE`, the same as a shown recovery phrase). Closing the picker,
  by choosing or cancelling, counts as activity.
- Screen lock, sleep and lost protection still lock at once; Android
  background expiry keeps its existing pick deferral.
- Rejected: no limit while a picker is open (on Linux without lock detection
  an abandoned picker would keep the vault open indefinitely); keeping 60 s
  (the reported bug); resetting only on return (the lock fires before it).

## Implementation

1. `controller::PickerOpen` guard: `Controller::picker_open()` counts an open
   picker in `State::pickers` and records activity; dropping it in the same
   lock generation decrements and records activity. `State::invalidate`
   resets the count, so a guard from before a lock changes nothing.
2. `snapshot()` uses `PICKER_IDLE` while `pickers > 0`.
3. Linux/Windows `documents::pick_path` holds the guard on its worker thread
   while the picker runs.
4. Android `request_pick` stores the guard in `PICK_OPEN`; the lifecycle
   poller drops it once the pick has an outcome and the activity is back, or
   the pick was forgotten. Never dropped with the state lock held.
5. Test `editor_tests::an_open_file_picker_lengthens_the_inactivity_lock`.
6. Docs: 0009 §1.8 Inactivity and Changelog, `CHANGELOG.md`.

## Execution log

- 2026-11-15: steps 1–6 done. Focused checks: the two inactivity tests pass;
  `cargo clippy -p qv-app --features desktop --all-targets -D warnings` and
  `cargo fmt --check` clean. Pending: Android build and phone check, full
  required gates, fresh-context review.
