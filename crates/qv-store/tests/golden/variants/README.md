# Additional 1.0 golden files

Created once on 2026-11-16 by the test-only `generate_1_0_golden_variants`
tool (plan 0065), from copies of the original `../1.0.qvault` and anchor.
Original frozen files were not changed. Never regenerate these variants.
Both use the original published golden password and retain its 13 records.

- `schema4`: explicitly remove the source's snapshot, then use the test-only
  `make_v4` helper to remove the snapshot table and sign the matching head
  and anchor. Exercises supported schema `0x7FFF0004` at generation 1.
- `generation2`: run the ordinary verified `prepare_rotation` and retain its
  generation-2 replacement with the matching external anchor. Exercises new
  object IDs, nonces, generation keys and rewrapped password descriptor.

`golden_legacy_schema_and_rotated_generation_open_unchanged` pins these hashes,
opens and fully verifies each file without changing its bytes or anchor. It
runs on Linux/Android and Windows with the store tests.

| File | SHA-256 |
|------|---------|
| `schema4/1.0.qvault` | `c5fae5ab583ab61189613abdadac8da39489023a0f74dce18cc492847608400a` |
| `schema4/anchors/dd99ac246edb5022eb6a57ab2690be42.anchor` | `1c43a3aa92a45e9f49a710f130564b78484f51ec0acccfd6f217bf89bda5e3e4` |
| `generation2/1.0.qvault` | `6666f0641f47fc821ef3548523a47b781792bdb58e0bb66cc31e1f951b95f96a` |
| `generation2/anchors/dd99ac246edb5022eb6a57ab2690be42.anchor` | `27efbf34dc7b3d70e5e5c02d14f1b34be11655054dd5aceb08d14280f5949bbc` |
