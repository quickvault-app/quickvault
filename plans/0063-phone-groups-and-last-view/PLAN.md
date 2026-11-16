# Plan 0063 — Groups on the phone, and the last group remembered

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Reviewed 2026-11-16 (see REVIEW.md) — phone check pending | 2026-11-15 | 2026-11-15 (owner request; owner chose the encrypted per-device note; details delegated, standing 2026-11-02) | 0002 §3.3 (amended), 0009 §1.8 (amended) |

## Intent

The owner, in QA: (1) the phone has no way to filter by group; (2) the app
should remember the selected group when a vault is reopened. Asked for
options, the owner chose storing the group's random ID in the app's own data,
encrypted, and asked for both at once.

## Findings

- Groups are reachable only from the desktop sidebar. "All items"
  (`Browse(None)`) shows no group chips; a group shows its subgroups. The
  phone has no sidebar, and at 980 px or less the sidebar is an icon rail
  that hides groups.
- `prefs` holds no vault data, name or path, by design. Group record IDs are
  stored readable in the vault's index, so a plain ID outside the vault
  would link the two files.
- Purpose keys are HKDF-SHA-512 subkeys of VK with vault ID salt and
  `["qv/purpose/v1", label, key_generation, derivation_profile]` info
  (0002 §3.3), typed by purpose in `qv-crypto`.

## Design (owner choice; details delegated, standing 2026-11-02)

1. **Phone groups.** The shell passes its top-level group rows to the list
   pane. On "All items", a chip row lists them (scrolling sideways), shown
   only where the sidebar does not list groups (app width 980 px or less).
   A chip opens the group, which already shows its subgroups.
2. **Last group, this run.** The workspace remembers the group screen last
   opened per vault (record ID, in memory) and lands there on unlock.
3. **Last group, across restarts: a device-view note.**
   - New purpose label `device-view` (0002 §3.3 amendment), allowed to
     MAC and AEAD, derived like every purpose key.
   - `Session::device_view_tag()`: HMAC (MacProfile V1) under that key of
     the fixed message `qv/device-view/tag/v1`, hex. Stable per vault and
     key generation; without VK it neither identifies the vault nor links
     to its file.
   - `Session::seal_device_view(note)` / `open_device_view(sealed)`:
     stored-data AEAD (XChaCha20-Poly1305, StoredDataProfile V1) under
     the same key, AAD `qv/device-view/note/v1`, notes at most 64 bytes,
     sealed at most 256 bytes. Opening failure is an ordinary error.
   - The app keeps `views` beside `prefs`: at most 16 lines
     `<tag hex>=<sealed hex>`, most recent first, bounded parse, 0600
     atomic write (`prefs::write_private`), unreadable or malformed lines
     ignored. The note is the group's 32-hex record ID, or empty for "All
     items".
   - On unlock, the app opens this vault's note; a live group lands there,
     anything else (missing, deleted, key rotated, bad data) lands on "All
     items". Navigating to a group or to "All items" updates the note.
   - Leakage: the file shows how many vaults this device remembers (up to
     16) and when the file changed; nothing else without VK.

Rejected: plain IDs in `prefs` (links to the vault file); a per-device area
inside the vault file (a format change, and it travels with file copies and
exports); in-memory only (Android closes background apps).

## Spec basis

0002 §3.3 amended (purpose `device-view`); 0009 §1.8 amended (phone group
chips; the last group per vault, encrypted per-device note).

## Steps

1. Specs, plan index, `CHANGELOG.md`.
2. `qv-crypto`: purpose `DeviceView` with AEAD and MAC; KAT label test.
3. `qv-core`: `device_view` module and tests (round trip, other vault and
   other key generation refuse, tampering refuses, bounds).
4. `qv-app`: `views` file (bounded parse/encode, tests); restore on unlock
   and record on navigation; in-memory fallback.
5. `qv-app` UI: top-level group chips on "All items" at narrow widths.
6. Focused checks; APK for the owner.

## Execution log

- 2026-11-15: plan written.
- 2026-11-15: steps 1–5 implemented. `qv-crypto`: `DeviceView` purpose
  (AEAD + MAC); label, distinctness and binding tests extended. `qv-core`:
  `device_view` (tag, seal, open; nonce stored before the ciphertext);
  round trip, fresh nonces, tampering and bounds tested; binding to vault,
  generation and VK is the purpose key's (test fixtures share one vault, so
  it is tested in `qv-crypto`). `qv-app`: `last_view` (`views` file, bounded
  parse, 16 entries, tests); restore in `enter_workspace` (no flash of "All
  items"), record from `pump` when the shown group changes, file written
  outside the state lock (lock order state, then views); test
  `the_last_group_reopens_after_unlock` covers lock, a restart, "All items"
  and a deleted group. UI: top-level chips at 980 px or less. Not done:
  `crates/qv-crypto/tests/vectors/crosscheck.py` has no `device-view`
  vector (the KAT file has none either).
- 2026-11-15: owner QA: the chips vanished inside a group. They now show on
  "All items" and in every group, led by an "All items" chip, with the
  current top-level group marked (`chip on`, `aria-pressed`).
- 2026-11-15: owner QA: no "All items" chip; tapping the marked chip again
  returns to "All items". A tapped chip kept the touch "hover" style
  (white text over the marked colour): hover styles now apply only under
  `(hover:hover)`, and `.chip.on` keeps its colour when hovered.
- 2026-11-15: owner QA: no way to create a group on the phone (the sidebar's
  "+" is desktop-only). A dashed "New group" chip ends the row, shown even
  when the vault has no groups.
- 2026-11-15: owner asked what desktop can do that phones cannot. Audit of
  hidden desktop surfaces (`.titlebar`, `.side`, `.list-head`, `.row-menu`,
  `desk-only`): covered on phones were lock/switch vault and theme
  (Settings), item edit and actions (phone bar), selection (hold), new
  item (FAB) and conflicts (list note). Missing, now added: the group
  actions menu (shared `group_actions` in the phone head), group colours
  on the chips (owner report), Trash (Settings, Vault). Still missing:
  type filters (Logins, Cards, …) and the full search of usernames and
  websites (command bar); proposed to the owner.
- 2026-11-15: owner QA: notes cut long words (`.notes` lacked
  `overflow-wrap:anywhere`) and could not be copied (app.js cancels
  `copy`/`cut` so the clipboard goes through Rust with auto-clear): new
  `CopyKey::Notes` and a Copy button on the current version's Notes card.
  Card rows showed user/key copy buttons (first plain and first protected
  field): they now show one copy button with a menu of the card's fields
  (names and icons only, no values), each copied as `CopyKey::Field`;
  quick-button styles scoped to direct buttons so the menu keeps its own.
- 2026-11-15: owner report: right after a successful fingerprint scan (no
  restart, no new fingerprint, no failed scan) the app said "Fingerprint
  unlock is off until you unlock with your master password." That message
  means the scan released the key and a later step refused it, which then
  invalidates the enrollment; the step was not recorded. Core now records
  `QuickFailure` (Keystore, Open, Anchor, Binding, Platform) and the message
  names it. Leading hypothesis, unconfirmed: the binding includes the
  password-keyslot version, a synced record, so a sync that brings another
  keyslot version (the owner synced and resolved a conflict that day)
  would read as Binding. Next occurrence's message decides. Test
  `notes_copy_through_the_app_clipboard` added.
- 2026-11-15: it recurred on the new build with no reason sentence, so the
  refusal happened before the prompt step (the reason was recorded only
  from there). Pre-prompt steps now record reasons too (Platform,
  NotEnrolled, OtherVault, Unreadable), both messages carry the reason, and
  "didn't work" vs "off" is decided by the live enrollment
  (`quick_ready`) rather than by re-reading the vault header
  (`quick_ready_for`), which could fail transiently and wrongly say "off".
- 2026-11-15: owner: fingerprint unlock worked on this build. Root cause not
  confirmed; the likeliest was the transient header read misreported as
  "off". A recurrence now names its step.
- 2026-11-15: owner (desktop screenshot): the card copy menu opened behind
  the following rows and the sidebar: nested in the row's `.quick` (a z-index 1
  layer), it could not rise above later rows. It is now a direct row child
  (`.card-menu`, z-index 40, like `.row-menu` but shown on phones too).
- 2026-11-15: row copy controls per type (owner agreed): login keeps
  username + password; card and other use the field menu (other: key for
  protected, globe for URLs, copy otherwise); Wi-Fi a password button; note
  a "Copy notes" button (an empty note says there is nothing to copy,
  `AppError::NothingToCopy`).
- 2026-11-15: owner rule: the menu is for any non-standard layout (a
  custom field) or many fields (cards); no fields, no button; labels quote
  the field name. The index and entry summaries now carry each copyable
  field's name, kind and protection (`CopyInfo`/`CopyField`, counted in
  view and page bytes; no protected value), and `RowCopy` (None, Pair,
  Secret, Menu) is decided per row from them (`row_copy`, unit-tested).
  The notes row button was dropped (no fields, no button); the Notes card
  keeps its Copy. `quick_fields` prefers a non-URL field as the username.
- 2026-11-15: qv-core lib: 156 passed, 1 failed (`index_record_states` pins an
  entry's index bytes; the copyable names add 27). Updated it to count them and
  to check the layout (names, protection, no protected values); index
  tests pass. APK built and installed; Gradle daemon stopped.
- 2026-11-15: owner: right-clicking a note still offered "Copy password". The
  row menu now follows `RowCopy` too: username/password for a standard
  login, password for a standard Wi-Fi, the quoted field list for a menu
  layout, and no copy items without fields.
- 2026-11-15: owner: hiding a login's username lost its copy button (and could
  make it the "password"). Standard login and Wi-Fi fields are now matched by
  name (Username, Password), whatever their hidden setting, in `row_copy` and
  `quick_fields`; test extended. A row with no visible field showed the
  plural sidebar label ("Logins"); it now shows the singular type ("Login").
