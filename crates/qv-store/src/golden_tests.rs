// SPDX-License-Identifier: AGPL-3.0-or-later

//! The store structures as frozen at 1.0 (spec 0015 §4, §6; plan 0064).
//!
//! Each sample is built from fixed values (MACs under public BIP-39 test
//! phrases), so its canonical bytes are deterministic. They must equal
//! `tests/golden/encodings.txt`, and each decoder must accept the golden bytes
//! and re-encode them unchanged. Never regenerate the file: a mismatch is a
//! format change, which needs a new format version (0015 §1.2).

use qv_crypto::{KeyGeneration, RecordId, StoredDataProfile, VaultId, VaultKey};
use qv_model::{Clock, ObjectId, RandomId, Timestamp, VersionId, WriterId};

use crate::archive::{ArchiveHeader, SnapshotKind};
use crate::head::{Anchor, LocalHead};
use crate::header::Header;
use crate::manifest::{Manifest, ObjectRef, VersionRef};
use crate::object::ObjectEnvelope;
use crate::receipts::ArchiveReceipts;
use crate::rotation_journal::{RotationJournal, RotationJournalBody};
use crate::snapshot_index::{SnapshotIndex, SnapshotRow};
use crate::snapshot_inventory::{InventoryRow, SnapshotInventory};

// The frozen samples live here, not in other modules' test fixtures, so
// that editing those fixtures never touches the golden bytes (review of plan
// 0064, finding 7). They are copies of the fixtures the 1.0 file was made
// from; never edit them.

/// A fixed UUIDv4-shaped ID: `n`, then the version and variant bits.
fn v4(n: u8) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0] = n;
    b[6] = 0x40;
    b[8] = 0x80;
    b
}

fn anchor() -> Anchor {
    Anchor {
        vault_id: VaultId(v4(1)),
        key_generation: KeyGeneration(1),
        local_seq: 3,
        local_digest: [4; 32],
    }
}

fn rid(n: u8) -> RecordId {
    RecordId(v4(n))
}

fn id16(n: u32) -> [u8; 16] {
    let mut b = v4(0);
    b[0..4].copy_from_slice(&n.to_be_bytes());
    b
}

fn oid(n: u32) -> ObjectId {
    ObjectId::from_bytes(id16(n)).unwrap()
}

fn version(n: u32, clock: &[(u32, u64)]) -> VersionRef {
    let mut pairs: Vec<_> = clock
        .iter()
        .map(|&(w, c)| (WriterId::from_bytes(id16(w)).unwrap(), c))
        .collect();
    pairs.sort();
    VersionRef::new(
        VersionId::from_bytes(id16(n)).unwrap(),
        Clock::from_pairs(pairs).unwrap(),
        oid(n),
    )
}

fn reference(n: u32) -> ObjectRef {
    ObjectRef::golden(oid(n), StoredDataProfile::V1, 100, [7; 32])
}

/// Control records 1–3, each with one head; record 4 with a head and two
/// history versions. Objects are numbered by the version they hold.
fn manifest() -> Manifest {
    let header = Header::new_vault(VaultId(v4(0x31)));
    let mut m = Manifest::empty(&header, rid(1), rid(2), rid(3)).unwrap();
    for n in 1..=4u8 {
        let k = u32::from(n);
        m.add_record(rid(n), version(k, &[(1, u64::from(n))]), reference(k))
            .unwrap();
    }
    m.replace_heads(rid(4), version(40, &[(1, 10)]), reference(40))
        .unwrap();
    m.replace_heads(rid(4), version(41, &[(1, 11), (2, 1)]), reference(41))
        .unwrap();
    m.validate().unwrap();
    m
}

fn keys() -> (VaultKey, VaultKey) {
    // Public BIP-39 encoding vectors: all-zero and all-one entropy.
    let zero = format!("{}art", "abandon ".repeat(23));
    let ones = format!("{}vote", "zoo ".repeat(23));
    (
        VaultKey::from_recovery_phrase(zero.as_bytes()).unwrap(),
        VaultKey::from_recovery_phrase(ones.as_bytes()).unwrap(),
    )
}

fn head(snapshot: bool, remote: bool) -> LocalHead {
    LocalHead {
        vault_id: VaultId(v4(1)),
        key_generation: KeyGeneration(1),
        local_seq: 2,
        previous_digest: Some([0xdd; 32]),
        manifest_hash: [0xee; 32],
        remote: remote.then_some(([0x55; 32], 9)),
        archive_receipts_digest: [0xab; 32],
        lan_links_digest: [0xcd; 32],
        snapshot_index_digest: snapshot.then_some([0xef; 32]),
    }
}

/// Each sample's name, its bytes, and the bytes its decoder re-encodes from
/// a given input.
type Reencode = fn(&[u8]) -> Vec<u8>;

fn samples() -> Vec<(&'static str, Vec<u8>, Reencode)> {
    let (vk, next) = keys();
    let header = Header::new_vault(VaultId(v4(0x0a)));
    let envelope = ObjectEnvelope {
        object_id: ObjectId::from_bytes(v4(0x0b)).unwrap(),
        key_generation: KeyGeneration(1),
        data_profile: StoredDataProfile::V1,
        nonce: [0x0c; 24],
        ciphertext: vec![0x0d; 17],
    };
    let mut index = SnapshotIndex::default();
    for (n, kind) in [(1u8, SnapshotKind::Ordinary), (2, SnapshotKind::Recovery)] {
        index
            .insert(SnapshotRow {
                id: [n; 16],
                created_at: Timestamp(1_700_000_000_000 + i64::from(n)),
                kind,
                manifest_hash: [n; 32],
                reference_digest: [n + 0x10; 32],
            })
            .unwrap();
    }
    index
        .set_retained(vec![
            ObjectId::from_bytes(v4(0x21)).unwrap(),
            ObjectId::from_bytes(v4(0x22)).unwrap(),
        ])
        .unwrap();
    let mut receipts = ArchiveReceipts::new();
    receipts.remember(b"/b/archive.qvsnap", [0x61; 32]).unwrap();
    receipts.remember(b"/a/archive.qvsnap", [0x62; 32]).unwrap();
    let inventory = SnapshotInventory::sign(
        VaultId(v4(0x11)),
        KeyGeneration(2),
        vec![InventoryRow {
            archive_id: [0x71; 16],
            generation: KeyGeneration(1),
            created_at: Timestamp(1_700_000_000_000),
            size: 4096,
            sha256: [0x72; 32],
        }],
        &next,
    )
    .unwrap();
    let journal = RotationJournal::sign(
        RotationJournalBody {
            operation_id: v4(0x81),
            vault_id: VaultId(v4(0x82)),
            replica_id: [0x83; 16],
            old_generation: KeyGeneration(1),
            new_generation: KeyGeneration(2),
            old_sequence: 7,
            new_sequence: 8,
            old_digest: [0x84; 32],
            new_digest: [0x85; 32],
            suffix: [0x86; 16],
            snapshot_id: [0x87; 16],
            inventory_digest: [0x88; 32],
        },
        &vk,
        &next,
    )
    .unwrap();
    vec![
        ("header", header.encode().unwrap().to_vec(), |b| {
            Header::decode(b).unwrap().encode().unwrap().to_vec()
        }),
        ("local_head", head(true, false).encode().unwrap(), |b| {
            LocalHead::decode(b).unwrap().encode().unwrap()
        }),
        (
            "local_head_schema_4",
            head(false, false).encode().unwrap(),
            |b| LocalHead::decode(b).unwrap().encode().unwrap(),
        ),
        (
            "local_head_remote",
            head(true, true).encode().unwrap(),
            |b| LocalHead::decode(b).unwrap().encode().unwrap(),
        ),
        ("anchor", anchor().encode().unwrap(), |b| {
            Anchor::decode(b).unwrap().encode().unwrap()
        }),
        ("object_envelope", envelope.encode().unwrap(), |b| {
            ObjectEnvelope::decode(b, 1024).unwrap().encode().unwrap()
        }),
        ("manifest", manifest().encode().unwrap().to_vec(), |b| {
            Manifest::decode(b).unwrap().encode().unwrap().to_vec()
        }),
        (
            "archive_header",
            ArchiveHeader::golden(&vk)
                .unwrap()
                .encode()
                .unwrap()
                .to_vec(),
            |b| {
                let h = ArchiveHeader::decode(b).unwrap();
                h.verify_mac(&keys().0).unwrap();
                h.encode().unwrap().to_vec()
            },
        ),
        (
            "snapshot_index",
            index.encode().unwrap().expose_secret().to_vec(),
            |b| {
                let i = SnapshotIndex::decode(b).unwrap();
                i.encode().unwrap().expose_secret().to_vec()
            },
        ),
        (
            "archive_receipts",
            receipts.encode().unwrap().expose_secret().to_vec(),
            |b| {
                let r = ArchiveReceipts::decode(b).unwrap();
                r.encode().unwrap().expose_secret().to_vec()
            },
        ),
        ("snapshot_inventory", inventory.encode().unwrap(), |b| {
            SnapshotInventory::decode(b)
                .unwrap()
                .verify(VaultId(v4(0x11)), KeyGeneration(2), &keys().1)
                .unwrap()
                .encode()
                .unwrap()
        }),
        ("rotation_journal", journal.encode().unwrap(), |b| {
            RotationJournal::decode(b).unwrap().encode().unwrap()
        }),
    ]
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn store_structures_match_the_frozen_golden_encodings() {
    let golden: Vec<(&str, &str)> = include_str!("../tests/golden/encodings.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_once(' ').unwrap())
        .collect();
    let samples = samples();
    let actual: Vec<String> = samples
        .iter()
        .map(|(n, b, _)| format!("{n} {}", hex(b)))
        .collect();
    let expected: Vec<String> = golden.iter().map(|(n, h)| format!("{n} {h}")).collect();
    assert_eq!(actual, expected, "actual:\n{}", actual.join("\n"));
    for ((name, _, reencode), (_, h)) in samples.iter().zip(&golden) {
        let bytes = unhex(h);
        assert_eq!(reencode(&bytes), bytes, "{name} does not re-encode");
    }
}

// 0012 §1: Budget inconsistency
#[test]
fn worst_case_manifest_matches_the_frozen_vector() {
    // The worst-case manifest at the 1.0 budgets (0004 §4.2): built from
    // fixed IDs, so its length and SHA-256 are fixed. The file holds them
    // instead of the 60 MiB encoding. The manifest is built by the
    // manifest module's `worst_case`, which needs the private fields; any
    // change to it that changes the bytes fails here.
    let golden: Vec<(&str, &str)> = include_str!("../tests/golden/worst-case-manifest.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_once(' ').unwrap())
        .collect();
    let bytes = crate::manifest::tests::worst_case().encode().unwrap();
    let sealed = qv_crypto::sealed_len(bytes.len()).unwrap();
    assert!(sealed <= crate::limits::MAX_MANIFEST_CIPHERTEXT);
    let sha = qv_crypto::object_hash(qv_crypto::ObjectHashProfile::V1, &bytes).0;
    let actual = vec![
        format!("plaintext_length {}", bytes.len()),
        format!("sealed_length {sealed}"),
        format!("sha256 {}", hex(&sha)),
    ];
    let expected: Vec<String> = golden.iter().map(|(n, v)| format!("{n} {v}")).collect();
    assert_eq!(actual, expected, "actual:\n{}", actual.join("\n"));
    assert_eq!(Manifest::decode(&bytes).unwrap().encode().unwrap(), bytes);
}

/// Restore using only the frozen self-contained archive and its password,
/// without opening a source vault or relying on its key/anchor (0064 finding 7).
// 0012 §1: Exchange-file password guessing
#[test]
fn golden_self_contained_archive_restores_without_its_source() {
    use crate::fs::test_dir::TestDir;
    use crate::{AnchorDir, ArchiveScope, VaultFile};
    use std::io::Cursor;
    let bytes = include_bytes!("../tests/golden/1.0.qvsnap");
    assert_eq!(
        hex(&qv_crypto::object_hash(qv_crypto::ObjectHashProfile::V1, bytes).0),
        "2abb3ca769b334e1bac19b4fcfe57bdfc18aa02f75a60b199cb2399307cffe0d"
    );
    assert_eq!(&bytes[..8], b"QVSNAP\0\x01");
    let n = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let h = ArchiveHeader::decode(&bytes[12..12 + n]).unwrap();
    let slot = qv_model::Keyslot::decode(h.password_slot.as_ref().unwrap()).unwrap();
    let password = qv_crypto::MasterPassword::from_utf8(
        b"quickvault 1.0 golden vault, published test password",
    )
    .unwrap();
    let vk = slot
        .slot
        .unlock(&password, h.vault_id(), h.key_generation())
        .unwrap();
    let (d, a) = (TestDir::new(), TestDir::new());
    let path = d.join("restored.qvault");
    let anchors = AnchorDir::open(a.path()).unwrap();
    VaultFile::create_from_archive(
        &path,
        &mut Cursor::new(bytes),
        ArchiveScope::new(h.vault_id(), h.key_generation()),
        &vk,
        &anchors,
        &|| false,
    )
    .unwrap();
    let restored = VaultFile::open(&path)
        .unwrap()
        .unlock(&password, &anchors)
        .unwrap();
    assert_eq!(restored.local_seq(), 1);
    assert_eq!(restored.manifest().records().len(), 13);
    restored.verify_all(&|| false).unwrap();
    assert!(restored.snapshot_rows().is_empty());
}

#[test]
fn golden_legacy_schema_and_rotated_generation_open_unchanged() {
    use crate::fs::test_dir::{TestDir, write_file};
    use crate::{AnchorDir, VaultFile};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/variants");
    let password = qv_crypto::MasterPassword::from_utf8(
        b"quickvault 1.0 golden vault, published test password",
    )
    .unwrap();
    for (variant, schema, generation, vault_sha, anchor_sha) in [
        (
            "schema4",
            0x7fff0004,
            1,
            "c5fae5ab583ab61189613abdadac8da39489023a0f74dce18cc492847608400a",
            "1c43a3aa92a45e9f49a710f130564b78484f51ec0acccfd6f217bf89bda5e3e4",
        ),
        (
            "generation2",
            0x7fff0005,
            2,
            "6666f0641f47fc821ef3548523a47b781792bdb58e0bb66cc31e1f951b95f96a",
            "27efbf34dc7b3d70e5e5c02d14f1b34be11655054dd5aceb08d14280f5949bbc",
        ),
    ] {
        let original = root.join(variant);
        let bytes = std::fs::read(original.join("1.0.qvault")).unwrap();
        let anchor = "dd99ac246edb5022eb6a57ab2690be42.anchor";
        let baseline = std::fs::read(original.join("anchors").join(anchor)).unwrap();
        let hash = |b: &[u8]| hex(&qv_crypto::object_hash(qv_crypto::ObjectHashProfile::V1, b).0);
        assert_eq!(hash(&bytes), vault_sha, "{variant}");
        assert_eq!(hash(&baseline), anchor_sha, "{variant}");
        let (d, a) = (TestDir::new(), TestDir::new());
        write_file(&d.join("1.0.qvault"), &bytes, 0o600);
        write_file(&a.join(anchor), &baseline, 0o600);
        let anchors = AnchorDir::open(a.path()).unwrap();
        let vault = VaultFile::open(&d.join("1.0.qvault")).unwrap();
        assert_eq!(vault.schema_version(), schema);
        assert_eq!(vault.header().key_generation.0, generation);
        let vault = vault.unlock(&password, &anchors).unwrap();
        assert_eq!(vault.manifest().records().len(), 13);
        vault.verify_all(&|| false).unwrap();
        drop(vault);
        assert_eq!(std::fs::read(d.join("1.0.qvault")).unwrap(), bytes);
        assert_eq!(std::fs::read(a.join(anchor)).unwrap(), baseline);
    }
}

/// One-time creation tool, never a qualification test. It refuses an existing
/// output directory and never modifies or regenerates the original goldens.
#[test]
#[ignore = "one-time golden variant generator; requires QV_GOLDEN_VARIANTS_OUT"]
fn generate_1_0_golden_variants() {
    use crate::vault::anchor_of;
    use crate::{AnchorDir, VaultFile};
    let output = std::path::PathBuf::from(std::env::var_os("QV_GOLDEN_VARIANTS_OUT").unwrap());
    assert!(!output.exists());
    crate::create_private_dirs(&output).unwrap();
    let original = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let password = qv_crypto::MasterPassword::from_utf8(
        b"quickvault 1.0 golden vault, published test password",
    )
    .unwrap();
    for variant in ["schema4", "generation2"] {
        let dir = output.join(variant);
        crate::create_private_dirs(&dir).unwrap();
        let anchors = AnchorDir::open(&dir.join("anchors")).unwrap();
        for entry in std::fs::read_dir(original.join("anchors")).unwrap() {
            let entry = entry.unwrap();
            crate::fs::test_dir::write_file(
                &anchors.path().join(entry.file_name()),
                &std::fs::read(entry.path()).unwrap(),
                0o600,
            );
        }
        let source = dir.join("source.qvault");
        crate::fs::test_dir::write_file(
            &source,
            &std::fs::read(original.join("1.0.qvault")).unwrap(),
            0o600,
        );
        let mut vault = VaultFile::open(&source)
            .unwrap()
            .unlock(&password, &anchors)
            .unwrap();
        let target = dir.join("1.0.qvault");
        if variant == "schema4" {
            let ids: Vec<_> = vault.snapshot_rows().iter().map(|r| r.id).collect();
            let mut write = vault.write().unwrap();
            write.remove_snapshots(&ids);
            write.commit().unwrap();
            crate::snapshot_tests::make_v4(&mut vault);
            drop(vault);
            std::fs::rename(&source, &target).unwrap();
        } else {
            let prepared = vault.prepare_rotation(&password, &|| false).unwrap();
            crate::fs::test_dir::write_file(
                &target,
                &std::fs::read(prepared.replacement.file.path()).unwrap(),
                0o600,
            );
            anchors
                .store(
                    &prepared.replacement.state.replica.replica_id,
                    &anchor_of(&prepared.replacement.state),
                )
                .unwrap();
            drop(prepared);
            std::fs::remove_file(source).unwrap();
        }
        VaultFile::open(&target)
            .unwrap()
            .unlock(&password, &anchors)
            .unwrap()
            .verify_all(&|| false)
            .unwrap();
    }
}
