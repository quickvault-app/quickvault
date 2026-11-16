# 0015 — Format Registry (frozen at 1.0)

| Status | Created | Depends on |
|--------|---------|------------|
| Accepted | 2026-11-16 | 0002 §1, §3; 0003; 0004 §1–§6; 0007 §3; 0012 §3 |

## Summary

QuickVault 1.0 freezes its persisted formats exactly as they were written by
the last development build (owner, 2026-11-16, plan 0064 Decision 1 C: "We are
not changing anything"). This spec is the registry of every persisted file
constant, table and CBOR structure: what a 1.0 build writes, every later build
must read. The owning specs keep the semantics; this registry pins the
numbers, keys and types. Golden vectors in `crates/qv-store/tests/golden/`
(plan 0064) hold the byte-exact encodings and the reference vault and archive.

Out of scope: the LAN wire protocol (0013, 0014; frozen after its protocol
review), server formats (0006, not planned) and app-local device files
(preferences, the vault list, quick-unlock state, the last-view file), which
are per-device state that fails safe to defaults.

## Requirements

### 1. Rules for every format

1.1. Every CBOR structure here is deterministic canonical CBOR (0003 §1):
shortest integer and length forms, definite lengths, map keys in canonical
order, no duplicate keys, no tags, no floats. A structure that does not
re-encode to the identical bytes is rejected.

1.2. Integer keys and array positions are never reused or renumbered. A
changed meaning, type or requirement of an existing key needs a new format
version of the structure.

1.3. Structures marked *closed* reject unknown keys (security configuration
and integrity bindings). Structures marked *open* preserve unknown keys on
rewrite (0003): a new optional field takes a new key and older builds keep it.

1.4. Every version value below is matched exactly; any other value fails
closed before further parsing (0004 §1, §6). The `0x7FFF` prefix of some 1.0
values is historical and carries no meaning beyond the value. A later version
of a structure takes the next unused value and is added here, with the read
rules for older values, before it is written.

1.5. Every bound in the owning spec (0004 §4) applies before allocation. The
values here are the 1.0 values; raising a bound that older builds enforce is a
format change.

### 2. File constants

| Item | 1.0 value | Owning spec |
|------|-----------|-------------|
| Vault file | SQLite database, extension `.qvault`, page size 4096 | 0004 §1 |
| `PRAGMA application_id` | `0x51564C54` ("QVLT") | 0004 §1.2 |
| `PRAGMA user_version` (schema) | `0x7FFF0005`; `0x7FFF0004` (no `snapshot_index`) is read and upgraded by the first commit that needs the index | 0004 §2, §2.5 |
| Header `format` | `0x7FFF0004` | 0004 §2 |
| Manifest `format` | `0x7FFF0002` | 0004 §3 |
| Anchor `format` | `0x7FFF0001` | 0005 §5.4 |
| Archive magic and version | `"QVSNAP\0"`, `0x01`; snapshot files `<32 hex>.qvsnap`, `recover-<32 hex>.qvsnap` | 0004 §6; 0007 §3 |
| Snapshot inventory `format` | `1` | 0007 §3.2 |
| Snapshot index row and plaintext version | `1` | 0004 §2.5 |
| Rotation journal `format` | `1` | 0002 §3.5.4 |
| Archive receipts plaintext version | `1` | 0004 §2.3 |
| Keyslot row type | `0` password slot; other types unsupported | 0002 §3.2 |
| Crypto profile IDs | every storage profile in 0002 §1.2 is `1` (password-kdf/1: Argon2id v1.3, 256 MiB, t=3, p=4); the LAN profiles (`lan/2`, `lan-pake/1`) are out of scope | 0002 §1 |
| Fixed derivation profile | `1` for the archive header-MAC key, the snapshot inventory key and the rotation journal keys (none of them carries a profile field) | 0002 §3.3 |
| Domain labels | `qv/header-mac/v1`, `qv/record/v1`, `qv/chunk/v1`, `qv/manifest/v1`, `qv/config/v1`, `qv/local-head/v1`, `qv/lan-links/v1`, `qv/archive-header/v1`, `qv/archive-receipts/v1`, `qv/snapshot-index/v1`, `qv/snapshot-inventory/v1`, `qv/rotation-journal/v1`; the key-hierarchy labels and HKDF purposes of 0002 §3 (pinned by `crates/qv-crypto/tests/vectors/qv-dev-keys.txt`) | 0002 §3 |
| Anchor files | `<32 hex replica id>.anchor` in the anchor directory | 0005 §5.4 |
| Rotation files | `<vault>.qvrotate` (the journal, also the marker), `<vault>.qvrotate-new-<32 hex>`, `<vault>.qvrotate-old-<32 hex>`; names containing `.qvrotate` are reserved | 0002 §3.5.4 |
| Temporary files | `.<name>.tmp-<16 hex>` and its `-journal` beside the final file | 0004 §1.1 |

### 3. Physical schema (DDL)

Exactly these tables and the two indexes SQLite makes for them
(`sqlite_autoindex_object_1`, `sqlite_autoindex_manifest_1`); no other
schema objects (0004 §1.2, §2). Schema `0x7FFF0004` is the same without
`snapshot_index`.

```sql
CREATE TABLE header (id INTEGER PRIMARY KEY CHECK (id = 1), body BLOB NOT NULL, mac BLOB NOT NULL) STRICT
CREATE TABLE keyslot (slot_id BLOB PRIMARY KEY CHECK (length(slot_id) = 16), type INTEGER NOT NULL, body BLOB NOT NULL) STRICT, WITHOUT ROWID
CREATE TABLE object (key_generation INTEGER NOT NULL, object_id BLOB NOT NULL CHECK (length(object_id) = 16), data_profile INTEGER NOT NULL, nonce BLOB NOT NULL CHECK (length(nonce) = 24), ciphertext BLOB NOT NULL, PRIMARY KEY (key_generation, object_id)) STRICT
CREATE TABLE manifest (manifest_hash BLOB PRIMARY KEY CHECK (length(manifest_hash) = 32), key_generation INTEGER NOT NULL, data_profile INTEGER NOT NULL, nonce BLOB NOT NULL CHECK (length(nonce) = 24), ciphertext BLOB NOT NULL) STRICT
CREATE TABLE local_head (id INTEGER PRIMARY KEY CHECK (id = 1), body BLOB NOT NULL, mac BLOB NOT NULL) STRICT
CREATE TABLE replica (id INTEGER PRIMARY KEY CHECK (id = 1), replica_id BLOB NOT NULL CHECK (length(replica_id) = 16), writer_id BLOB NOT NULL CHECK (length(writer_id) = 16), next_counter INTEGER NOT NULL) STRICT
CREATE TABLE archive_receipts (id INTEGER PRIMARY KEY CHECK (id = 1), sealed BLOB NOT NULL) STRICT
CREATE TABLE lan_link (link_id BLOB PRIMARY KEY CHECK (length(link_id) = 16), key_generation INTEGER NOT NULL, sealed BLOB NOT NULL) STRICT, WITHOUT ROWID
CREATE TABLE snapshot_index (id INTEGER PRIMARY KEY CHECK (id = 1), sealed BLOB NOT NULL) STRICT
```

`lan_link` is retired (0014): every commit writes the empty set and open
checks the stored rows against head key 8.

### 4. Store structures

Types: `bstr N` is a byte string of exactly N bytes; `uint (u16)`/`(u32)` the
range; `int (ms)` milliseconds since the Unix epoch.

**4.1. Header body** (closed; all keys required; 0004 §2). MAC/1 under the
`header-mac` purpose key over `["qv/header-mac/v1", header_body,
[[slot_id, type, body], …]]`.

| Key | Field | CBOR |
|-----|-------|------|
| 0 | format | uint, `0x7FFF0004` |
| 1 | vault_id | bstr 16 (UUIDv4) |
| 2 | key_generation | uint (u32, ≥ 1) |
| 3 | data_profile | uint (u16, stored-data) |
| 4 | derivation_profile | uint (u16) |
| 5 | mac_profile | uint (u16) |
| 6 | min_data_profile | uint (u16, stored-data) |
| 7 | min_derivation_profile | uint (u16) |

**4.2. Password keyslot** (`keyslot` row type 0; the body is the canonical
descriptor of the password-keyslot record, §5.7; open: unknown keys in
existing rows are preserved and covered by the header MAC). Exactly one
password slot.

**4.3. Object envelope** (0004 §2.2): array `[object_id bstr 16,
key_generation uint, data_profile uint, nonce bstr 24, ciphertext bstr]`; the
`object` table stores the same five values. Record AAD: `["qv/record/v1",
vault_id, key_generation, record_id, version_id, object_id, data_profile]`.

Attachment chunks are objects too. A chunk is encrypted under its
attachment key with AAD `["qv/chunk/v1", vault_id, key_generation,
attachment_record_id, object_id, chunk_index, data_profile]`.

**4.4. Manifest** (closed; all keys required; 0004 §3). Encrypted with
stored-data under the `manifest` purpose key, AAD `["qv/manifest/v1",
vault_id, key_generation, data_profile]`; envelope `[key_generation,
data_profile, nonce, ciphertext]`; `manifest_hash` is SHA-256 of the canonical
envelope. `config_digest` is SHA-256 of `["qv/config/v1", format, vault_id,
key_generation, min_data_profile, min_derivation_profile]`, where `format` is
the **header** format (`0x7FFF0004`), not the manifest's;
`reference_digest` is SHA-256 of the canonical reference array.

| Key | Field | CBOR |
|-----|-------|------|
| 0 | format | uint, `0x7FFF0002` |
| 1 | config_digest | bstr 32 |
| 2 | control | `[vault_meta_id, keyslot_id, root_group_id]` |
| 3 | records | `[[record_id, heads, history, floor], …]`, strictly ascending by record ID |
| 4 | references | `[[object_id, data_profile, ciphertext_length, sha256], …]`, strictly ascending by object ID |

A version reference is `[version_id, clock, object_id]`, plus a fourth element
`[chunk_object_id, …]` for an `attachment_meta` version, or `true` for a
permanent tombstone. `heads` (1 to 256) and `history` are strictly ascending
by version ID; `floor` is a clock (empty: no floor).

**4.5. Local head** (closed; MAC/1 under the `local-state` purpose key with
the label `qv/local-head/v1`; 0005 §5.4).

| Key | Field | CBOR |
|-----|-------|------|
| 0 | vault_id | bstr 16 |
| 1 | key_generation | uint (u32, ≥ 1) |
| 2 | local_seq | uint (≥ 1) |
| 3 | previous_digest | bstr 32; absent iff `local_seq` = 1 |
| 4 | manifest_hash | bstr 32 |
| 5 | remote_checkpoint_id | bstr 32, optional (server mode; never written) |
| 6 | remote_sequence | uint, present iff key 5 is |
| 7 | archive_receipts_digest | bstr 32 |
| 8 | lan_links_digest | bstr 32 |
| 9 | snapshot_index_digest | bstr 32; present iff the schema is `0x7FFF0005` |

`archive_receipts_digest` and `snapshot_index_digest` are SHA-256 of the
stored blob. `lan_links_digest` is SHA-256 of `["qv/lan-links/v1",
[[link_id, SHA-256(sealed)], …]]` over the rows (the empty set in 1.0).

**4.6. Anchor file** (closed; 0005 §5.4).

| Key | Field | CBOR |
|-----|-------|------|
| 0 | format | uint, `0x7FFF0001` |
| 1 | vault_id | bstr 16 |
| 2 | key_generation | uint (u32, ≥ 1) |
| 3 | local_seq | uint (≥ 1) |
| 4 | local_digest | bstr 32 |

**4.7. Archive receipts** (0004 §2.3): `archive_receipts.sealed` is the
envelope `[1, data_profile uint, nonce bstr 24, ciphertext bstr]`, sealed
under the `local-state` key with AAD `["qv/archive-receipts/v1", vault_id,
key_generation, replica_id, data_profile]`; plaintext `[1, [[path bstr, digest
bstr 32], …]]`, strictly ascending by path, at most 4,096 rows of paths up to
4,096 bytes.

**4.8. Snapshot index** (0004 §2.5): row `[1, data_profile uint, nonce bstr
24, ciphertext bstr]`, sealed under the `local-state` key with AAD
`["qv/snapshot-index/v1", vault_id, key_generation, data_profile]`; plaintext
`[1, rows, retained]`, rows `[[snapshot_id bstr 16, created_at int, kind uint
(0 ordinary, 1 recovery), manifest_hash bstr 32, reference_digest bstr 32],
…]` strictly ascending by snapshot ID; retained `[object_id bstr 16, …]`
strictly ascending.

**4.9. Archive** (0004 §6): `"QVSNAP\0" || u8 0x01 || u32be header_length ||
header || frame(manifest envelope) || frame(object envelope)*`, each frame a
`u32be` length and that many bytes; extension `.qvsnap`. Header (closed; every
key except 10 required). The header MAC is MAC/1 under the `header-mac`
purpose key, derived for the header's vault ID and key generation with
derivation profile 1 (the header has no field for it), over
`["qv/archive-header/v1", header_body]`, the stored map without key 11:

| Key | Field | CBOR |
|-----|-------|------|
| 0 | archive_id | bstr 16 |
| 1 | vault_id | bstr 16 |
| 2 | key_generation | uint (u32, ≥ 1) |
| 3 | data_profile | uint (u16, stored-data) |
| 4 | minimum_profiles | `[min_data_profile, min_derivation_profile]` |
| 5 | created_at | int (ms) |
| 6 | manifest_hash | bstr 32 |
| 7 | reference_digest | bstr 32 |
| 8 | object_count | uint |
| 9 | total_length | uint |
| 10 | password_slot | optional bstr: canonical working keyslot descriptor |
| 11 | header_mac | bstr 32 |

**4.10. Snapshot inventory** (closed; file
`inventory-<32 hex vault id>-<8 hex generation>.qvinv`; MAC/1 under the
generation's `local-state` key over `["qv/snapshot-inventory/v1", body]`, body
being keys 0–3; 0007 §3.2).

| Key | Field | CBOR |
|-----|-------|------|
| 0 | format | uint, 1 |
| 1 | vault_id | bstr 16 |
| 2 | generation | uint (u32, ≥ 2) |
| 3 | rows | `[[archive_id bstr 16, generation uint, created_at int, size uint, sha256 bstr 32], …]`, strictly ascending by archive ID |
| 4 | mac | bstr 32 |

**4.11. Rotation journal** (closed; at most 4,096 bytes; 0002 §3.5.4). Keys
13 and 14 are the old- and new-generation MACs, each MAC/1 under that
generation's `local-state` key over `["qv/rotation-journal/v1", body]`, body
being keys 0–12.

| Key | Field | CBOR |
|-----|-------|------|
| 0 | format | uint, 1 |
| 1 | operation_id | bstr 16 (UUIDv4) |
| 2 | vault_id | bstr 16 |
| 3 | replica_id | bstr 16 |
| 4 | old_generation | uint (≥ 1) |
| 5 | new_generation | uint, old + 1 |
| 6 | old_sequence | uint (≥ 1) |
| 7 | new_sequence | uint, old + 1 |
| 8 | old_digest | bstr 32 |
| 9 | new_digest | bstr 32, differs from old |
| 10 | suffix | bstr 16 |
| 11 | snapshot_id | bstr 16 |
| 12 | inventory_digest | bstr 32 |
| 13 | old_mac | bstr 32 |
| 14 | new_mac | bstr 32 |

### 5. Record structures (0003)

**5.1. Record** (open: unknown keys are preserved; an unknown kind fails
closed).

| Key | Field | CBOR | |
|-----|-------|------|---|
| 0 | kind | uint: 0 vault_meta, 1 group, 2 entry, 3 attachment_meta, 4 password_keyslot | required |
| 1 | version_id | bstr 16 (UUIDv4) | required |
| 2 | clock | array of `[writer_id, counter]` pairs, nonempty | required |
| 3 | modified_at | int (ms) | required |
| 4 | deleted | bool: a final tombstone | required |
| 5 | payload | map of the kind's payload | present iff `deleted` is false |

Payloads are open (§1.3): unknown keys are preserved on rewrite.

**5.2. Vault metadata** (kind 0).

| Key | Field | CBOR | |
|-----|-------|------|---|
| 0 | name | text | required |
| 1 | description | text | optional |
| 2 | root_group | bstr 16 | required |
| 3 | settings | map | required |

Settings (open; every key optional): 0 `history_depth` (uint, default 10,
0007 §1.1), 1 `trash_retention_days` (uint, default 30, 0007 §2.1), 2
`generator` (map, 0010 §5.1), 3 `recovery_skipped_at` (int ms, 0002 §4.2).
Optional keys are absent when the value is absent, never `null`.

The generator defaults (settings key 2) are a map whose key 0 is the mode and
whose other keys depend on it (every key of the mode required; unknown keys
kept; an unreadable value is kept unchanged):

| Mode (key 0) | Keys |
|--------------|------|
| 0 characters | 1 length, 2 lowercase, 3 uppercase, 4 digits, 5 symbols, 6 exclude_ambiguous, 7 exclude_chars (text), 8 require_each_class |
| 1 passphrase | 1 words, 2 wordlist (0 = eff-large), 3 separator (text, one character), 4 capitalize (0 none, 1 first letter), 5 append_digit |
| 2 PIN | 1 length |

**5.3. Group** (kind 1).

| Key | Field | CBOR | |
|-----|-------|------|---|
| 0 | parent | bstr 16, or `null` (root only) | required |
| 1 | name | text | required |
| 2 | notes | text | optional |
| 3 | icon | text | optional |
| 4 | order | text (fractional index) | required |
| 5 | deleted_at | int (ms) | optional: soft delete |
| 6 | delete_operation | bstr 16 UUIDv4 | optional, only with deleted_at |

**5.4. Entry** (kind 2).

| Key | Field | CBOR | |
|-----|-------|------|---|
| 0 | group | bstr 16 | required |
| 1 | type_hint | text | required |
| 2 | title | text | required |
| 3 | fields | array of field maps (§5.5) | required |
| 4 | notes | text | optional |
| 5 | icon | text | optional |
| 6 | expires_at | int (ms) | optional |
| 7 | created_at | int (ms) | required |
| 8 | modified_at | int (ms) | required |
| 9 | deleted_at | int (ms) | optional: soft delete |
| 10 | delete_operation | bstr 16 UUIDv4 | optional, only with deleted_at |

**5.5. Entry field** (all keys required; IDs unique within an entry).

| Key | Field | CBOR |
|-----|-------|------|
| 0 | id | bstr 16 |
| 1 | name | text |
| 2 | kind | uint: 0 text, 1 secret, 2 url, 3 email, 4 totp, 5 multiline, 6 date; other values are kept |
| 3 | value | text or bstr (at most 256 KiB) |
| 4 | protected | bool |

**5.6. Attachment metadata** (kind 3; all keys required; `chunk_count`
equals the list length).

| Key | Field | CBOR |
|-----|-------|------|
| 0 | entry | bstr 16 |
| 1 | name | text |
| 2 | mime | text |
| 3 | size | uint (at most 2 GiB, 0004 §4) |
| 4 | chunk_count | uint |
| 5 | chunk_object_ids | array of bstr 16, unique |
| 6 | content_hash | bstr 32: SHA-256 of the whole plaintext |

**5.7. Password keyslot** (kind 4; all keys required; an unknown profile ID
fails closed).

| Key | Field | CBOR |
|-----|-------|------|
| 0 | slot_id | bstr 16 |
| 1 | kdf_profile | uint (u16) |
| 2 | kdf_salt | bstr 32 |
| 3 | wrap_profile | uint (u16) |
| 4 | kek_commit | bstr 32 |
| 5 | nonce | bstr 24 |
| 6 | wrapped_vk | bstr 48 |

### 6. Golden vectors

`crates/qv-store/tests/golden/` holds, never regenerated after 1.0: canonical
sample encodings of the §4 structures, the worst-case manifest's length and
SHA-256 at the 0004 §4.2 budgets, and a reference vault (with its anchor
file) and archive with their published password and expected contents.
`crates/qv-model/tests/golden/records.txt` holds the §5 record encodings. Every build must decode, re-encode and
open them unchanged. The crypto vectors in `crates/qv-crypto/tests/vectors/`
(except the LAN files) are frozen with them.

## Security Considerations

Freezing removes no check: every value is still matched exactly, closed
structures still reject unknown keys, and authentication still precedes
parsing (0004 §6). Keeping the development-era numbers changes no security
property; they are only identifiers.

## Open Questions

None.

## Changelog

- 2026-11-16: Created, Accepted (owner freeze go-ahead 2026-11-15; Decision 1
  C 2026-11-16; plan 0064). Same day, after the fresh review: chunk, LAN-link
  and receipt layouts and labels, anchor, rotation and temporary file names,
  the fixed derivation profile, the SQLite auto-indexes and the
  config-digest format added; profile scope and one integer type corrected.
