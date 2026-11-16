# Review — Plan 0064: Format freeze for 1.0

| Verdict | Reviewed | Diff reviewed |
|---------|----------|---------------|
| Changes requested | 2026-11-16 | uncommitted working tree on deb5e5d (`git diff` plus untracked golden files, tests, generator, spec 0015, release notes) |

## Summary

Spec 0015 matches the code for every key number, type and required/optional
status that it lists. I checked it against the header, head, anchor,
manifest, archive, snapshot index, inventory and rotation-journal codecs, the
receipt store, the schema DDL and every qv-model payload decoder. The version
bump touches only workspace crate versions in both lockfiles (21 lines). The
`qv info` note removal and the visibility changes are correct. The main
problem is the golden tests. The store golden tests do pin the closed
structures. The record golden test does not pin the meaning of optional keys
or enum codes, and the binary golden files are not hash-pinned. The registry
also leaves out several persisted constants it claims to cover.

## Plan conformance

| Step | Done | Notes |
|------|------|-------|
| 1 Specs | yes | 0015 written; 0004/0002/0012 amended; gaps below |
| 2 Wording | yes | some development wording remains (finding 8) |
| 3 Encoding vectors | partial | store: good; records: the round trip does not pin optional keys or codes (finding 1) |
| 4 Worst case | yes | length, sealed length and hash pinned; decode/re-encode checked |
| 5 Phone benchmark | not done | owner-delegated skip (finding 5) |
| 6 Golden files | partial | no hash pin; self-contained restore not exercised (findings 2, 7) |
| 7 Crypto vectors | yes | status lines; `device-view` vector correct (CBOR `84 6d… 6b "device-view" 01 01`) |
| 8 Docs/version | yes | release notes overclaim (finding 8) |

Out-of-scope changes: `.github/workflows/release.yml` (release-notes body)
and `docs/release-notes/1.0.0.md` belong to plan 0056 step 8. They are
logged there.

## Findings

1. **[high] The record golden test does not pin optional keys or enum codes** — `crates/qv-model/src/payload/tests.rs:683-831`
   The samples are hand-built test bytes compared with `records.txt`.
   Production code only runs `round_trip`. Payload maps are open, so unknown
   keys are kept verbatim (`k => keep(...)`). If entry `notes` is renumbered
   from 4 to 11, or `icon`, `expires_at`, a group's `notes`, any settings
   key, or the generator mode or wordlist codes change, the golden key comes
   back as an extension and re-encodes byte for byte. If `FieldKind` codes
   are swapped (Url 2 ↔ Email 3, Multiline, Date) in both directions, the
   round trip is also unchanged. The golden vault does not catch this either:
   `describe` (`tests/common/mod.rs:66`) prints no field kind, and the vault
   has no icon, expiry or settings values. Failure scenario: a 1.x build
   reads 1.0 URL fields as e-mails (autofill matching breaks), or ignores a
   1.0 user's history depth or trash retention, and every golden test still
   passes. Fix: decode each golden record and assert the typed values (kind
   codes, protected flags, notes, icon, expiry, deletion, settings,
   generator), and assert that `ext` is empty at every level. Print
   `f.kind` in `describe`. Optionally build the samples from typed structs
   with the production encoder, as `golden_tests.rs` does.

2. **[medium] The golden binary files are not hash-pinned** — `crates/qv-core/tests/golden.rs:99,155`
   `before` is the hash of the working copy, so the test only proves that
   opening does not rewrite it. If `1.0.qvault`, `1.0.qvsnap` or the anchor
   are regenerated after a format change, the test passes as long as the
   contents match, and the change shows up only as an opaque binary diff.
   Fix: assert the README SHA-256 values for all three files before copying.

3. **[medium] The registry leaves out persisted constants** — `specs/0015-format-registry.md:51-66, 165-168, 201-236`
   The following are absent: the `qv/chunk/v1` chunk AAD and chunk layout
   (0004 §3.3); `qv/lan-links/v1` (the head key 8 digest in every file); the
   receipt envelope `[1, profile, nonce, ct]` with its `local-state` key and
   AAD `[label, vault_id, key_generation, replica_id, profile]`; the anchor
   file name `<32 hex replica>.anchor`; the rotation names `<vault>.qvrotate`
   and `-new-`/`-old-`; the fixed derivation profile 1 for inventory and
   journal keys; and bounds (inventory 1 MiB and 4,096 rows, 256 snapshots,
   2^21 retained objects, a 48 MiB index, the 64 KiB archive header, the
   anchor and head caps, 4,096 fields, 1 MiB records). 0004 §1.1 now says
   that every constant is "registered in 0015". Failure scenario: a 1.x
   change to anchor or `.qvrotate` naming passes a registry check and orphans
   1.0 anchors (writes blocked), or breaks recovery of an interrupted 1.0
   rotation. Fix: add these items.

4. **[medium] `config_digest` names the wrong `format`** — `specs/0015-format-registry.md:122`; code `crates/qv-store/src/manifest.rs:465`
   The sentence appears in the manifest section, where `format` reads as
   `0x7FFF0002`. The code hashes the header format `0x7FFF0004`. Fix: write
   "header `format` (`0x7FFF0004`)".

5. **[medium, escalation] The KDF and budgets were frozen without the required benchmark** — `specs/0012-security-validation.md` §3; `specs/0002-cryptography.md` §1.3
   0012 §3 ("Benchmark KDF and resource budgets on the supported targets")
   was amended under delegation to "host measurements only". The 0002 §1.3
   MUST (about 1 s on mid-range Android, at most 2 s low-end) stays
   unverified. The step's own Done-when ("the owner has confirmed") is not
   met. Relaxing a release gate in a spec should be the owner's explicit
   call. Fix: the owner confirms, or runs `budget_bench` P0–P3 and a timed
   unlock on the phone before tagging.

6. **[low] Registry inaccuracies** — `specs/0015-format-registry.md:65, 70, 264`
   §2 says "every profile in 0002 §1.2 is `1`", but `lan/2` exists. §3 says
   "no other schema objects", but the two `sqlite_autoindex_*` rows are part
   of the exact schema (`schema.rs`). §5.2 gives `recovery_skipped_at` as
   uint, but `read_time` accepts an int.

7. **[low] Coverage gaps** — `crates/qv-core/tests/golden.rs:8,159-163`
   - The self-contained `1.0.qvsnap` is only merged into its source vault.
     Restoring it as a new vault with the password (key 10) is never
     exercised.
   - The test is Linux/Android only, so it never runs on Windows.
   - There is no golden file for a rotated (generation 2) vault or for a
     schema `0x7FFF0004` vault.
   - The store samples depend on other modules' test fixtures
     (`anchor::tests::sample`, `manifest::tests::sample`,
     `header::tests::v4`). Editing those fixtures invites regenerating
     `encodings.txt`. Move frozen samples into `golden_tests.rs`.

8. **[low] Stale wording and overclaims**
   - `crates/qv-crypto/src/kat.rs:173` ("unstable until 1.16") and `:345`:
     the generator still writes the old "regenerated freely" status line.
   - The `crosscheck.py` docstring still says "development vectors" and
     offers `--write` to regenerate frozen files.
   - The plan's step 7 text still says to rename the files to `qv-v1-*`.
   - `docs/release-notes/1.0.0.md` says the golden files "enforce this".
     That is true only after findings 1 and 2 are fixed.

Checked and correct: the header (8 keys), head (keys 0–9 with the 3/5/6/9
rules), anchor, archive header (0–11, 10 optional, the MAC transcript
without 11, derivation profile 1), inventory, rotation journal (0–14), the
snapshot index row and plaintext, receipt plaintext, object envelope and
record AAD, manifest keys and AAD, the DDL (byte-identical to `schema.rs`),
application ID, page size and schema/header/manifest/anchor values, and
every record and payload key, field kind code and generator key in qv-model.
The store golden samples use distinct values per field, so a changed key,
type or order in those closed structures fails. `anchor::{encode,decode}`
are `pub(crate)` and `ArchiveHeader::golden` is `cfg(test)`. Neither widens
the public API.

## Fixes applied by reviewer

None (instructed not to edit).

## Gates

Full gates not run (shared machine, disk 95% full). Focused:
`cargo test --locked -j 1 -p qv-model records_match` passed (1 test). The
other golden tests were not run by the reviewer.

## Documentation

- [x] CHANGELOG.md 1.0.0 section
- [x] ARCHITECTURE.md, AGENTS.md wording
- [x] Spec Changelogs and `specs/README.md` row
- [ ] Plan status and `plans/README.md` row (left to the fixing agent, per instruction)

## Escalations for the owner

- Finding 5: confirm freezing password-kdf/1 and the budgets without a
  phone measurement, or run it first.
- Findings 1–3 should be fixed before tagging 1.0. Once the golden files
  ship, gaps in what they pin cannot be closed for 1.0 files.

## Developer response (2026-11-16)

- Finding 1 (high), fixed: `golden_meanings` decodes every record sample and
  checks each field lands where 0015 says, with no unknown keys at any
  level; the golden vault test checks the Mail entry's field kinds and
  flags.
- Finding 2 (medium), fixed: `golden_files_are_the_1_0_files` checks the
  committed files against the README's SHA-256 values.
- Findings 3, 4 and 6, fixed in 0015: the chunk, LAN-link and receipt layouts
  and labels, anchor, rotation and temporary file names, the fixed
  derivation profile, the auto-indexes, the config digest's header format,
  the storage-profile scope and `recovery_skipped_at`'s type.
- Finding 8, fixed: `kat.rs` comment and generator status line,
  `crosscheck.py` docstring and `--write` note, the plan's step 7 text.
- Finding 5: left for the owner (release decision).
- Finding 7 (low): open.

## Follow-up repairs — plan 0065 (2026-11-16)

Finding 7 is implemented: the frozen self-contained archive restores using only
its published password, without a source vault; original golden core tests now
run on Windows as well. Additional hash-pinned schema-0x7FFF0004 and generation-2
vault/anchor variants are created once from copies of the original goldens and
fully verified without rewriting their bytes. The original frozen files are
unchanged. Fixed store encoding fixtures were already present in the inherited
patch and are retained. The one-time variant generator is explicitly ignored
and requires a new named output directory; it is not a skipped qualification.

Finding 5 remains a release-evidence/owner item: no phone KDF measurement was
made in this session. See 0065 for current gates and known limitations.
