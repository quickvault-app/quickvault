# Plan 0064 — Format freeze for 1.0

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Reviewed 2026-11-16 (findings fixed, see REVIEW.md) | 2026-11-15 | 2026-11-16 | 0012 §3, 0004 §1, §2, §3, §4, §6, OQ1; 0002 §1.3, §1.4; new format registry spec |

Implements plan 0056 step 7. Owner go-ahead for the freeze: 2026-11-15.

## Intent

Make the 1.0 vault formats permanent: what a 1.0 build writes, every later
build must read. 0012 §3 requires, before the offline release, a frozen physical
DDL, integer-key CBOR registry, profile IDs and parameters, archive format, and
golden file vectors including the worst-case manifest at the maximum budgets
(0004 §4.2), with the KDF and resource budgets benchmarked on supported targets.

## Scope

- **In:** the formats exactly as they are today, version numbers included
  (Decision 1 C): vault header `0x7FFF0004`, schema (`user_version`)
  `0x7FFF0005` (with the existing upgrade from `0x7FFF0004`), manifest
  `0x7FFF0002`, anchor `0x7FFF0001` become the frozen 1.0 values; only the
  "development" wording goes. One registry spec for every
  persisted CBOR structure. Golden vectors: a 1.0 vault file and archive,
  per-structure canonical encodings, the worst-case manifest (length and hash),
  and the crypto vectors marked frozen. Password-kdf/1
  parameters and the 0004 §4 budgets confirmed by a phone benchmark (0002
  §1.3, 0004 OQ1).
- **Out:** the LAN wire protocol (`lan/2`, `lan-pake/1`): its vectors are frozen
  after the separate protocol review (plan 0056, D12) and the LAN release
  gate. App-local device files (`preferences`, the vault list,
  `quick-unlock`, `views`): per-device state that already fails safe to
  defaults, not vault data; unchanged. Server formats (not planned).
- Every other format stays as it is too: archive (`QVSNAP\0`,
  format `0x01`), rotation journal (`1`), snapshot inventory (`1`), snapshot
  index (`1`), application ID `0x51564C54` ("QVLT"), every crypto profile ID
  (`1`).

## Spec basis

| Behaviour | Spec | Spec status | Notes |
|-----------|------|-------------|-------|
| What must be frozen, golden vectors, benchmarks | 0012 §3 | Accepted | This plan |
| File version numbers, schema | 0004 §1.1, §2 | Accepted | Amended in step 1 |
| Manifest and anchor formats | 0004 §3 | Accepted | Amended in step 1 |
| Budgets and the worst-case manifest | 0004 §4.1–§4.2 | Accepted | Vector in step 4 |
| Benchmark budgets before freezing | 0004 OQ1 | Open Question | Decision 3; resolved only with the owner's phone data |
| Password-kdf/1 parameters | 0002 §1.3 | Accepted (candidate until benchmarked) | Decision 3 |
| Profile transitions registry | 0002 §1.4 | Accepted | Unchanged; registry spec cites it |
| CBOR registry | 0003, 0004 §3, module tables | Spread over code docs | New spec in step 1 |

## Decisions needed

1. **Development vaults. Decided 2026-11-16 by the owner: C.** "We are not
   changing anything." Existing vaults open in 1.0 unchanged. Options were:
   - **(A) Clean break (recommended):** 1.0 refuses them with a clear
     message ("made by a development build; open it with that build and copy
     the data over"). No development handling in frozen code. Matches the
     standing rule that pre-1.0 formats are volatile with no migration.
     Cost: the owner re-creates their vaults (archives from development
     builds embed the development manifest, so they do not import either).
   - **(B) One-time upgrade:** 1.0 also reads exactly the final development
     versions (identical layout) and rewrites them as release on the first
     write, like the schema 4→5 upgrade. Keeps the owner's data; adds
     development handling to the most sensitive read path, kept until removed
     in a later release.
   - **(C) Keep the `0x7FFF…` numbers as the 1.0 values.** No break, but every
     tool shows 1.0 vaults as "development format" and the numbering
     convention is lost.
2. **Release numbers.** Dropped with Decision 1 C: nothing is renumbered.
   After 1.0 the `0x7FFF` prefix has no meaning beyond its value; a later
   version of a format is the next number, allocated in the registry spec.
3. **KDF and budgets: the phone benchmark (owner's phone, plan 0056 step 5).**
   Keep password-kdf/1 at Argon2id 256 MiB, t=3, p=4 if unlock takes about
   one second on the owner's phone and at most two on the low-end target
   (0002 §1.3); otherwise lower memory or passes (never below the 64 MiB
   floor) and update the profile and vectors together before step 6. The
   same session runs `budget_bench` P0–P3 for 0004 OQ1. The owner confirms
   the result; the plan records it and closes OQ1.

## Design

**Version numbers.** Unchanged (Decision 1 C): the constants
(`header::FORMAT`, `schema::SCHEMA_VERSION`, `manifest::MANIFEST_FORMAT`,
`anchor::ANCHOR_FORMAT`) keep their values and checks, including the schema
`0x7FFF0004` → `0x7FFF0005` upgrade. Only wording changes: the CLI's `qv info`
development warning (`schema_version >> 16 == 0x7fff`) is removed, and doc
comments, the `qv-store` lib.rs table heading and the specs stop calling these
formats development or volatile. Nothing about the layouts or the read and
write paths changes: the freeze pins and records, it does not redesign.

**Registry spec.** A new spec, `specs/0015-format-registry.md` (status
Accepted when this plan is approved), lists every persisted CBOR structure
with its integer keys, types, required/optional and bounds, citing the owning
spec section, and every file-level constant (magic, version, application ID,
profile IDs). It is the normative copy; the module doc tables
(`qv-model/src/payload/*.rs`, `record.rs`, `qv-store/src/{header,head,anchor,
manifest,archive,snapshot_inventory}.rs`) stay as summaries pointing to it.
Rules: keys are never reused; a new optional field takes a new key; unknown
optional keys are preserved (as today, 0003); a changed meaning needs a new
format version.

**Golden vectors** (`crates/qv-store/tests/golden/`, committed):
- `encodings.txt`: for each structure, a fixed sample value and its canonical
  CBOR hex. A test decodes each, re-encodes it and compares bytes, so any key,
  type or ordering change fails.
- `worst-case-manifest.txt`: the canonical length and SHA-256 of
  `Manifest::worst_case()` built from fixed IDs (deterministic). No 64 MiB blob
  in the repository; the generator is the existing `worst_case_with`. The test
  asserts the length is at most 64 MiB (0004 §4.2) and the hash matches.
- `1.0.qvault`, `1.0.qvsnap` (a few hundred KiB): a vault and an archive made by
  the 1.0 code from a fixed script (two groups, a login with each field kind,
  a card, a note, an attachment, one snapshot, a resolved conflict, a trashed
  item), password and expected contents in `README.md`. Tests open both with
  the real password-kdf/1 (one derivation each, memoized), verify the
  authenticated contents against the expected list, and check that opening
  never rewrites them (hash unchanged). A generator, `cargo run -p qv-core
  --example golden_vault -- <dir>`, makes them once; later builds never
  regenerate them, they must keep opening them.
- Crypto: `crates/qv-crypto/tests/vectors/qv-dev-*.txt` keep their names
  (no churn); their status lines become "frozen at 1.0; never regenerated"
  (except the LAN files, out of scope); `crosscheck.py` gains the
  `device-view` label (plan 0063).

**Order matters:** golden vault files depend on password-kdf/1 parameters
(the keyslot), so they are generated only after Decision 3 is settled.

**Durability, secrets, dependencies:** no write path changes; no new
secrets; no new crates. The golden password is a published test password,
never a real one.

| Crate | Why | Alternatives | Review / audit status |
|-------|-----|--------------|-----------------------|
| None | | | |

## Steps

### Step 1 — Specs

- **Do:** write `specs/0015-format-registry.md` (every structure and
  constant above, rules for evolution). Amend 0004 §1.1, §2 and §3: the
  current numbers are the frozen 1.0 values (Decision 1 C), the schema
  upgrade text stays, "development"/"volatile" wording goes. Amend 0002 §1.3
  and 0004 OQ1 only in step 5, with the
  benchmark. Changelogs; `specs/README.md` index.
- **Files:** `specs/0015-format-registry.md`, `specs/0004-local-storage.md`,
  `specs/README.md`.
- **Done when:** every key in the module tables appears in 0015 with the same
  number and type; 0004 no longer calls a 1.0 format development or volatile
  outside its Changelog.
- **Verify:** `rg -n -i 'development|volatile' specs/0004-local-storage.md`
  (Changelog and history only).

### Step 2 — Release wording

- **Do:** remove the `qv info` development warning and its test expectation;
  reword the `qv-store` lib.rs table heading ("1.0 formats, frozen") and the
  format constants' doc comments. No constant, check or code path changes.
- **Files:** `crates/qv-store/src/{lib,header,schema,manifest,anchor}.rs`,
  `crates/qv-cli/src/cmd/info.rs` (and its test).
- **Done when:** `git diff` in these files touches comments and the CLI
  message only; an existing vault opens unchanged.
- **Verify:** `cargo test --locked -p qv-store`, `-p qv-cli`.

### Step 3 — Registry encoding vectors

- **Do:** `encodings.txt` and a test that round-trips each sample to identical
  bytes; every structure in 0015 has a sample.
- **Files:** `crates/qv-store/tests/golden/encodings.txt`,
  `crates/qv-store/tests/golden.rs`.
- **Done when:** changing any key number, type or field order breaks the test.
- **Verify:** `cargo test --locked -p qv-store --test golden`.

### Step 4 — Worst-case manifest vector

- **Do:** make `worst_case()` deterministic (fixed IDs), record its length and
  SHA-256 in `worst-case-manifest.txt`, test both and the 64 MiB bound.
- **Files:** `crates/qv-store/src/manifest.rs`,
  `crates/qv-store/tests/golden/worst-case-manifest.txt`.
- **Verify:** the existing `manifest_worst_case_fits_the_limit` and the new
  vector test.

### Step 5 — Phone benchmark (owner + agent)

- **Do:** with the phone connected: `budget_bench` P0–P3 and the KDF unlock
  time (plan 0053 tooling). Record results in this plan; amend 0002 §1.3
  (candidate → benchmarked, parameters) and close 0004 OQ1 with the owner's
  confirmation (Decision 3). If parameters change: update
  `PasswordKdfProfile::V1`, its vectors and `crosscheck.py` together.
- **Done when:** the owner has confirmed the parameters and budgets.

### Step 6 — Golden files

- **Do:** the `golden_vault` example; generate `1.0.qvault` and `1.0.qvsnap`
  once; `README.md` with the password and expected contents; tests that open,
  verify and leave them unchanged.
- **Files:** `crates/qv-core/examples/golden_vault.rs`,
  `crates/qv-store/tests/golden/{1.0.qvault,1.0.qvsnap,README.md}`, tests.
- **Verify:** `cargo test --locked -p qv-core golden`.

### Step 7 — Crypto vectors frozen

- **Do:** give the non-LAN `qv-dev-*.txt` files the frozen status line (names
  kept, see Revisions); update their loaders and `crosscheck.py` (and its
  `device-view` label).
- **Verify:** `cargo test --locked -p qv-crypto`;
  `python3 crates/qv-crypto/tests/vectors/crosscheck.py`.

### Step 8 — Documentation and handoff

- **Do:** plan 0056 step 7 points here and is ticked; `ARCHITECTURE.md`
  (formats frozen); `CHANGELOG.md`; `plans/README.md`. Fresh-context review
  (storage and crypto formats).

## Test plan

- Golden encodings, worst-case vector, golden vault/archive open and verify,
  unknown-version refusal (existing tests) (0012 §1 rows: malformed SQLite/CBOR; Snapshot disk growth via the
  golden snapshot).
- Final tree: all `AGENTS.md` gates, plus `cargo test --locked -p qv-app
  --features validation` and both qv-app clippy feature runs.

## Risks and stop conditions

- Stop if a structure turns out not to round-trip to identical bytes: that is
  a canonical-encoding bug to fix (with its own review) before freezing.
- Stop if the phone benchmark misses 0002 §1.3 at 64 MiB: the owner decides.
- Stop if any layout or version change seems needed: the freeze pins what
  exists (Decision 1 C); a change goes back to the owner first.
- Once step 6 has generated the golden files, nothing that changes their
  bytes may land without a new format version.

## Documentation

`specs/0015-format-registry.md` (new), `specs/0004-local-storage.md`,
`specs/0002-cryptography.md` (step 5), `specs/README.md`, `ARCHITECTURE.md`,
`CHANGELOG.md`, `crates/qv-store/src/lib.rs` table,
`crates/qv-crypto/tests/vectors/` headers, plan 0056 step 7,
`plans/README.md`.

## Revisions

- 2026-11-16: owner chose Decision 1 C (keep today's formats and numbers);
  Decision 2 dropped; step 2 reduced to wording; golden files named `1.0.*`;
  crypto vector files keep their names. Approved.

## Execution log

- 2026-11-15: plan written (Proposed).
- 2026-11-16: Decision 1 C (owner); revised and Approved.
- 2026-11-16: Step 1: spec 0015 written (every module table, DDL, labels,
  settings keys; checked against the codecs); 0004 §1.1, §2.3, §2.4, §3.4
  reworded, the stale "receipt schema 0x7FFF0003" sentence dropped, OQ1
  resolved; 0002 §1.3 and OQ1, 0012 §3, AGENTS.md, ARCHITECTURE.md updated.
- 2026-11-16: Step 2: `qv info` development note removed (and its test);
  "development value" comments in qv-store/qv-model reworded to frozen.
  Policy defaults in qv-core (snapshot counts, clock tolerance) keep their
  wording: they are behaviour, not format.
- 2026-11-16: Step 3: `qv-store` `golden_tests` (12 structures, built from
  fixed values with MACs under public BIP-39 test phrases) against
  `tests/golden/encodings.txt`; record versions of every kind in
  `qv-model` `records_match_the_frozen_golden_encodings` against
  `crates/qv-model/tests/golden/records.txt` (placed with the crate that
  owns the codec; 0015 §6 names both). Each decodes and re-encodes the
  golden bytes unchanged. Anchor `encode`/`decode` became `pub(crate)`;
  `ArchiveHeader::golden` is test-only.
- 2026-11-16: Step 4: `worst_case()` was already deterministic; its
  plaintext length (62,070,890), sealed length (62,914,576, under 64 MiB) and
  SHA-256 are in `worst-case-manifest.txt`.
- 2026-11-16: Step 5: no phone was connected. Owner-delegated (standing
  2026-11-02; owner "do everything that needs to be done for 1.0"):
  password-kdf/1 and the budgets stay as they are, recorded in 0002 §1.3 and
  0004 OQ1 as frozen without a measured phone run. A later change is a new
  profile.
- 2026-11-16: Step 6: `examples/golden_vault.rs` made `1.0.qvault` (200 KiB),
  its anchor file and `1.0.qvsnap` (145 KiB) once; committed under
  `crates/qv-store/tests/golden/` with a README (password, contents,
  hashes). `crates/qv-core/tests/golden.rs` opens a private copy, checks the
  contents, verify counts, history, the attachment bytes and the snapshot,
  that the file hash is unchanged after open and lock, and that the archive
  merges with no conflicts. The shared `describe` helper moved to
  `crates/qv-core/tests/common/`. No conflicted record in the golden vault
  (making one needs a second replica); multi-head manifests are covered by
  the codec tests only.
- 2026-11-16: Step 7: the four non-LAN `qv-dev-*.txt` files marked frozen;
  `crosscheck.py` adds the `device-view` purpose, its value derived by the
  Python standard library and matched by `kat_dev_keys…`; the OpenSSL
  Argon2id cross-check passes.
- 2026-11-16: Step 8: docs updated; version 1.0.0 (plan 0056 step 8);
  fresh-context review started (storage and crypto formats).
