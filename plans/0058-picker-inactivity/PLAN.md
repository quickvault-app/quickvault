# Plan 0058 — File picking and playback: inactivity lock, shared input folders

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented — APK build, phone check and fresh-context review pending | 2026-11-15 | 2026-11-15 (owner bug report; limit delegated by the owner, standing 2026-11-02) | 0009 §1.8 (amended), 0011 §4 (amended) |

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

## Part 2 — attaching from a folder others can change (owner decision)

The owner then could not attach an MP3 from a group-writable (0770) music
folder: "Check the input bounds, password …". Cause: 0011 §4 required every
ancestor of input *and* output paths to be owned by the user or root and not
group/other-writable; the app maps the refusal to the generic invalid-input
message. The owner decided (2026-11-15): "I'm importing a file into the vault.
What does it matter if its writeable by other users? It will be a readonly
copy inside the vault."

- 0011 §4 amended: the ancestor owner/writable rule applies to output only.
  Input keeps no-follow traversal, no `..`, regular files, the size bound and
  the identity/size/time checks while reading. Accepted residual: someone who
  can change the folder could swap which bytes are imported.
- `qv-platform` `files.rs` (`parent(path, owned)`) and `files_windows.rs`
  (`checked(path, owned)`, ACL rules only for output); unit tests show input
  from a shared folder opens and output there is refused.
- App test renamed `attachments_follow_path_rules_and_save_where_ancestors_are_safe`:
  adding always succeeds, saving follows the host's ancestors. The validation
  flow now requires the add to succeed and accepts a refused save where the
  fixture root is unsafe; `finish_fixture` probes output instead of input.
- Not changed: the misleading message for a refused *output* folder (could get
  its own error later).

## Part 3 — playing preview locks the vault (owner bug report)

The vault locked while an audio preview played. 0009 §1.8 amended: a playing
audio or video preview counts as activity; a paused one does not. `app.js`
reports `timeupdate` (capture phase; media fires it only while playing or
seeking) through the existing once-a-second `#qv-activity` path. Screen lock,
sleep and Android background expiry are unchanged. Accepted: unattended
playback keeps the vault open until the media ends (attachments are at most
10 MiB; the player does not loop).

## Execution log

- 2026-11-15: steps 1–6 done. Focused checks: the two inactivity tests pass;
  `cargo clippy -p qv-app --features desktop --all-targets -D warnings` and
  `cargo fmt --check` clean.
- 2026-11-15: parts 2 and 3 implemented (see above).
