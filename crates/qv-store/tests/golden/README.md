# QuickVault 1.0 golden files

Frozen at 1.0 (spec 0015 §6, plan 0064). Never regenerate or edit these
files: every later build must decode, open and verify them unchanged. A
change that breaks them is a format change and needs a new format version
(0015 §1.2).

| File | What | Checked by |
|------|------|------------|
| `encodings.txt` | Canonical CBOR of every store structure (0015 §4) | `qv-store` `golden_tests` |
| `worst-case-manifest.txt` | Length and SHA-256 of the worst-case manifest at the 1.0 budgets (0004 §4.2) | `qv-store` `golden_tests` |
| `1.0.qvault`, `anchors/` | A vault made by the 1.0 code, and its anchor file | `qv-core` `tests/golden.rs` |
| `1.0.qvsnap` | A self-contained archive exported from that vault | `qv-core` `tests/golden.rs` |

Record encodings (0015 §5) are in `crates/qv-model/tests/golden/records.txt`.

`1.0.qvault` and `1.0.qvsnap` were made by
`cargo run --release -p qv-core --example golden_vault -- <new dir>`. The
password is published and protects only test data:

```text
quickvault 1.0 golden vault, published test password
```

Contents (vault name "Golden vault"):

- group Work
  - entry Mail (work), login: Username, Password, Website, Email, One-time
    code, notes; attachment `readme.txt` (70,000 bytes, `i % 251`); two
    earlier titles kept as history
  - entry Wifi, wifi: Password
- group Home
  - entry Card, card: Cardholder, Number, Expires, CVV
  - entry Note, note: a multiline field
- in the trash: group Old with entry Legacy
- purged: entry Temporary (a permanent tombstone)
- one local snapshot inside the vault

SHA-256:

```text
55969343745c5ec471aade44fce82773379f1e03739cdb7e2169482d60a2dc32  1.0.qvault
2abb3ca769b334e1bac19b4fcfe57bdfc18aa02f75a60b199cb2399307cffe0d  1.0.qvsnap
f76f4a5d314d11913c7230766cbc609c7715a8d98daa0f8be7bc3cd0a57fb57f  anchors/dd99ac246edb5022eb6a57ab2690be42.anchor
```
