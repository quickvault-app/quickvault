# Plan 0064 — Format freeze for 1.0

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Proposed | 2026-11-15 | — | 0012 §3, 0004 §1, §2, §3, §4, §6, OQ1; 0002 §1.3, §1.4; new format registry spec |

Implements plan 0056 step 7. Owner go-ahead for the freeze: 2026-11-15.

## Intent

Make the 1.0 vault formats permanent: what a 1.0 build writes, every later
build must read. 0012 §3 requires, before the offline release, a frozen physical
DDL, integer-key CBOR registry, profile IDs and parameters, archive format, and
golden file vectors including the worst-case manifest at the maximum budgets
(0004 §4.2), with the KDF and resource budgets benchmarked on supported targets.

## Scope

- **In:** release version numbers for the formats still marked development
  (`0x7FFF…`): vault header `0x7FFF0004`, schema (`user_version`) `0x7FFF0005`,
  manifest `0x7FFF0002`, anchor `0x7FFF0001`. One registry spec for every
  persisted CBOR structure. Golden vectors: a 1.0 vault file and archive,
  per-structure canonical encodings, the worst-case manifest (length and hash),
  and the crypto vectors relabelled from development to frozen. Password-kdf/1
  parameters and the 0004 §4 budgets confirmed by a phone benchmark (0002
  §1.3, 0004 OQ1).
- **Out:** the LAN wire protocol (`lan/2`, `lan-pake/1`): its vectors are frozen
  after the separate protocol review (plan 0056, D12) and the LAN release
  gate. App-local device files (`preferences`, the vault list,
  `quick-unlock`, `views`): per-device state that already fails safe to
  defaults, not vault data; unchanged. Server formats (not planned).
- Formats already on release numbers stay as they are: archive (`QVSNAP\0`,
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

1. **Development vaults.** Vaults made by development builds (all existing
   ones, including the owner's desktop and phone vaults) carry `0x7FFF…`
   versions. Options:
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
   Recommendation: A, unless the owner has data to keep. Then B.
2. **Release numbers.** Recommendation (owner-delegated, standing
   2026-11-02): every renumbered format becomes `1`, matching the formats
   already released (archive, journal, inventory, index). Schema 1 is today's
   schema 5 (with `snapshot_index`); the schema 4 upgrade path goes (under A)
   or stays for development files only (under B).
3. **KDF and budgets: the phone benchmark (owner's phone, plan 0056 step 5).**
   Keep password-kdf/1 at Argon2id 256 MiB, t=3, p=4 if unlock takes about
   one second on the owner's phone and at most two on the low-end target
   (0002 §1.3); otherwise lower memory or passes (never below the 64 MiB
   floor) and update the profile and vectors together before step 6. The
   same session runs `budget_bench` P0–P3 for 0004 OQ1. The owner confirms
   the result; the plan records it and closes OQ1.

## Design

**Version numbers.** One constant per format (`header::FORMAT`,
`schema::SCHEMA_VERSION`, `manifest::MANIFEST_FORMAT`, `anchor::ANCHOR_FORMAT`)
becomes `1`. Opening checks each exactly; any other value fails closed before
further parsing (0004 §1, §6), with `StoreError::Unsupported` for unknown
numbers and a distinct `StoreError::DevelopmentFormat` for `0x7FFF…` values
so the app and CLI can say "made by a development build" (Decision 1 A). The
CLI's `qv info` development warning (`schema_version >> 16 == 0x7fff`) is
removed. Nothing about the layouts changes: the freeze renumbers and pins,
it does not redesign.

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
- `v1.qvault`, `v1.qvsnap` (a few hundred KiB): a vault and an archive made by
  the 1.0 code from a fixed script (two groups, a login with each field kind,
  a card, a note, an attachment, one snapshot, a resolved conflict, a trashed
  item), password and expected contents in `README.md`. Tests open both with
  the real password-kdf/1 (one derivation each, memoized), verify the
  authenticated contents against the expected list, and check that opening
  never rewrites them (hash unchanged). A generator, `cargo run -p qv-core
  --example golden_vault -- <dir>`, makes them once; later builds never
  regenerate them, they must keep opening them.
- Crypto: `crates/qv-crypto/tests/vectors/qv-dev-*.txt` are renamed
  `qv-v1-*.txt` with the status line "frozen at 1.0; never regenerated"
  (except the LAN files, out of scope); `crosscheck.py` follows and gains the
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
  constant above, rules for evolution). Amend 0004: release numbers in §1.1,
  §2 and §3 (header 1, schema 1 including `snapshot_index`, manifest 1,
  anchor 1), development files per Decision 1, drop the schema 4→5 upgrade
  text (Decision 1 A). Amend 0002 §1.3 and 0004 OQ1 only in step 5, with the
  benchmark. Changelogs; `specs/README.md` index.
- **Files:** `specs/0015-format-registry.md`, `specs/0004-local-storage.md`,
  `specs/README.md`.
- **Done when:** every key in the module tables appears in 0015 with the same
  number and type; no `0x7FFF` remains in 0004 except in the Changelog.
- **Verify:** `rg -n '0x7FFF' specs/0004-local-storage.md` (Changelog only).

### Step 2 — Release numbers

- **Do:** set the four constants to 1; `StoreError::DevelopmentFormat` for
  `0x7FFF…` header, schema, manifest and anchor values (Decision 1 A), with
  app and CLI messages; remove the schema 4 upgrade path and its tests, the
  `qv info` development warning, and update the lib.rs table. Under Decision
  1 B instead: accept exactly the final development values, rewrite on the
  first write transaction, test the upgrade, crash points and refusal of
  every other development value.
- **Files:** `crates/qv-store/src/{header,schema,manifest,anchor,error,lib,
  compact}.rs`, `crates/qv-cli/src/cmd/info.rs`, app error text.
- **Done when:** a new vault has all four versions at 1; a file with any
  `0x7FFF…` value is refused with `DevelopmentFormat` before key derivation
  where possible, else before manifest parsing; unknown values give
  `Unsupported`.
- **Verify:** `cargo test --locked -p qv-store`, `-p qv-core`, `-p qv-cli`.

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

- **Do:** the `golden_vault` example; generate `v1.qvault` and `v1.qvsnap`
  once; `README.md` with the password and expected contents; tests that open,
  verify and leave them unchanged.
- **Files:** `crates/qv-core/examples/golden_vault.rs`,
  `crates/qv-store/tests/golden/{v1.qvault,v1.qvsnap,README.md}`, tests.
- **Verify:** `cargo test --locked -p qv-core golden`.

### Step 7 — Crypto vectors frozen

- **Do:** rename the non-LAN `qv-dev-*.txt` files to `qv-v1-*.txt` with the
  frozen status line; update their loaders and `crosscheck.py` (and its
  `device-view` label).
- **Verify:** `cargo test --locked -p qv-crypto`;
  `python3 crates/qv-crypto/tests/vectors/crosscheck.py`.

### Step 8 — Documentation and handoff

- **Do:** plan 0056 step 7 points here and is ticked; `ARCHITECTURE.md`
  (formats frozen); `CHANGELOG.md`; `plans/README.md`. Fresh-context review
  (storage and crypto formats).

## Test plan

- Golden encodings, worst-case vector, golden vault/archive open and verify,
  development-file refusal (each of the four values), unknown-version
  refusal (0012 §1 rows: malformed SQLite/CBOR; Snapshot disk growth via the
  golden snapshot).
- Final tree: all `AGENTS.md` gates, plus `cargo test --locked -p qv-app
  --features validation` and both qv-app clippy feature runs.

## Risks and stop conditions

- Stop if a structure turns out not to round-trip to identical bytes: that is
  a canonical-encoding bug to fix (with its own review) before freezing.
- Stop if the phone benchmark misses 0002 §1.3 at 64 MiB: the owner decides.
- Stop if any layout change seems needed: the freeze renumbers, it does not
  redesign; a change goes back to the owner first.
- Once step 6 has generated the golden files, nothing that changes their
  bytes may land without a new format version.

## Documentation

`specs/0015-format-registry.md` (new), `specs/0004-local-storage.md`,
`specs/0002-cryptography.md` (step 5), `specs/README.md`, `ARCHITECTURE.md`,
`CHANGELOG.md`, `crates/qv-store/src/lib.rs` table,
`crates/qv-crypto/tests/vectors/` headers, plan 0056 step 7,
`plans/README.md`.

## Revisions

None.

## Execution log

- 2026-11-15: plan written (Proposed).
