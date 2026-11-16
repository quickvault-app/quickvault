# Review — Plan 0063: Groups on the phone, and the last group remembered

| Verdict | Reviewed | Diff reviewed |
|---------|----------|---------------|
| Changes requested (minor; no blocker) | 2026-11-16 | commits 0f70e84, 5b4dd6f, d4e2cc6, 82c9a19, deb5e5d (crypto and secret-handling parts only, as asked) |

## Summary

Fresh-context review limited to the crypto and secret-handling parts:
the `device-view` purpose, `qv-core` `device_view`, the app's `views` file
and how the controller uses it, the quick-unlock failure diagnostics, and the
index `CopyInfo` additions. The design matches the plan. The purpose key is
derived like every other purpose key, and notes are sealed with stored-data/1
under random 24-byte nonces. Seal and open are bounded before allocation, the
`views` parse is strict and bounded, the file is written 0600 with an
exclusive temporary file and a rename, and no secret reaches a message or a
log. The findings below are one spec conformance gap and some small leaks and
diagnostics gaps. UI parts (chips, copy menus) were not reviewed.

## Plan conformance

| Step | Done | Notes |
|------|------|-------|
| 1 Specs | yes | 0002 §3.3 label row and Changelog; 0009 §1.8 |
| 2 qv-crypto purpose | yes | `DeviceView` AEAD + MAC; label, distinctness and binding tests; KAT vector added later by plan 0064 |
| 3 qv-core device_view | yes | round trip, fresh nonce, tamper, bounds; binding left to qv-crypto test (logged) |
| 4 views file / restore | yes | bounded parse tests; `the_last_group_reopens_after_unlock` |
| 5 UI chips | not reviewed | out of the requested scope |
| 6 APK | owner | phone check pending |

Out-of-scope changes: none found in the reviewed parts. QuickFailure
diagnostics, CopyInfo and the copy menus were added during owner QA and are
logged.

## Findings

1. **[medium] Device-view AAD and tag message are not canonical CBOR transcripts (0002 §7.2)** — `crates/qv-core/src/device_view.rs:19-20`
   0002 §7.2 says "All AAD/transcripts use canonical CBOR arrays with
   purpose, vault, generation, IDs and relevant profiles". The note AAD is
   the raw bytes `qv/device-view/note/v1`, and the tag MAC message is the
   raw bytes `qv/device-view/tag/v1`. Today the binding comes from the key,
   which is derived per vault, generation and derivation profile, so I see no
   practical attack. But this is new crypto code that departs from a stated
   rule, and the stored-data profile is not bound at all. Fix: use the AAD
   `["qv/device-view/note/v1", vault_id, key_generation, data_profile]` and
   the message `["qv/device-view/tag/v1", vault_id, key_generation]`. The file
   is app-local and fails safe (0015 excludes it), so changing it now costs
   only the remembered groups. The alternative is to amend 0002 §7.2 with an
   explicit exception. Action: escalated (crypto code).

2. **[low] The sealed length shows "All items" versus a group** — `crates/qv-core/src/device_view.rs:52`
   Padmé framing gives 24+20 = 44 bytes (88 hex) for the empty note and
   24+52 = 76 bytes (152 hex) for a 32-hex ID. Anyone who reads `views`
   learns, for each remembered vault, whether it was last left on "All
   items". The plan's leakage section ("nothing else without VK") and 0009
   §1.8 do not mention this. Fix: seal a fixed-width note (for example, pad
   to 32 bytes with a length byte), or document the leak.

3. **[low] The tag is the same on every device for a vault** — `crates/qv-core/src/device_view.rs:40-44`
   The tag is HMAC(K_device-view, constant). It is the same on every device
   and copy of a vault, and for the life of a key generation. Someone holding
   the app data of two devices can tell that they hold the same vault. The
   plan's leakage section does not state this. Fix: document it, or mix a
   random per-install salt kept in `prefs` into the message.

4. **[low] Quick-failure attribution is untested, and I/O is reported as "Open"** — `crates/qv-core/src/quick_unlock.rs:184-219`
   No test asserts any `QuickFailure` value. The plan relies on "a
   recurrence now names its step", so a wrong label would mislead the
   diagnosis. After a successful scan, any `self.open` error (I/O, capacity,
   integrity) is recorded as `Open`, and the user sees "Its key no longer
   opens this vault." That is the transient-failure case the log names as the
   likely root cause. A cancellation seen at the binding check is likewise
   labelled `Binding`. The messages carry no secret or identifier. This
   finding is about accuracy only. Fix: map `StoreError::Io`/`StorageCapacity`
   to a separate reason (and consider not invalidating the enrollment on
   them). Add synthetic-backend tests for NotEnrolled, OtherVault, Binding
   and Open.

5. **[low] The tag goes stale after a key rotation inside a session** — `crates/qv-app/src/controller.rs:2270`, `:2317`
   `view_tag` is computed once in `enter_workspace`, but `seal_device_view`
   derives the key from the current header. If the session survives a
   rotation, notes are filed under the old generation's tag, sealed with the
   new key, and the next unlock falls back to "All items". This fails safe.
   Fix: recompute the tag in `record_view` (it is cheap), or clear `view_tag`
   when the generation changes.

Checked and found sound: `open_device_view` checks the length before it
splits or decrypts and maps every failure to `NotFound`. The plaintext is
returned in `SecretBytes`. The note is a record ID, which is not secret.
`Views::parse` bounds the file (9,248 bytes), checks lowercase hex and tag
length, drops duplicates, stops at 16 entries and ignores CR/odd/oversized
lines. `read_private` refuses links. Lock order is always state then views,
the session lock is never held while views is taken, and the file is written
outside both locks from the single UI `pump` caller. AppError quick messages
are `&'static str` with no vault data. `CopyInfo` carries names, kinds and
the protected flag only, the menu fetches field handles from the entry
detail, and protected values are revealed only on copy. Index and page byte
accounting add the name bytes in both `ViewData::bytes` and `Row::bytes`.

## Fixes applied by reviewer

None (instructed not to edit; another agent applies fixes).

## Gates

Not run (shared machine, low disk; the review was told to read code). No
focused test was run for this plan.

## Documentation

- [x] CHANGELOG.md
- [x] Spec Changelogs (0002, 0009)
- [ ] Plan status and `plans/README.md` row (left to the fixing agent, per instruction)

## Escalations for the owner

- Finding 1: either the device-view AAD/tag become canonical CBOR
  transcripts, or 0002 §7.2 gets an explicit app-local exception.

## Developer response (2026-11-16)

- Finding 1 (medium), fixed: the note AAD and the tag message are canonical
  CBOR arrays binding vault ID, key generation and (for the note) the data
  profile (0002 §7.2); 0002 §3.3 says so. Notes from earlier builds no
  longer open, so the app forgets the remembered group once.
- Finding 2 (low), fixed: notes are padded to one length before sealing; the
  test checks an empty note seals to the same size.
- Finding 3 (low), documented in 0002 §3.3.
- Findings 4 and 5 (low): open.

## Follow-up repairs — plan 0065 (2026-11-16)

- Finding 4: fixed after the 0002 §5.4 amendment. Cancelled operations and
  transient store I/O/capacity/busy failures retain enrollment and report the
  appropriate storage step. Synthetic backend tests cover NotEnrolled,
  OtherVault, Binding, Anchor, Open, post-release cancellation and busy-file
  storage failure; the error-classification test covers I/O and capacity too.
- Finding 5: fixed. `record_view` recomputes the current tag and checks both
  tag and note before deciding to skip a write. A failed seal changes neither
  remembered state nor persisted views.

See 0065 for current checks. Physical biometric/lifecycle evidence remains
separate; the source repairs are not a device-security claim.
