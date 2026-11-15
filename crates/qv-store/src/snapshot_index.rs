// SPDX-License-Identifier: AGPL-3.0-or-later

//! The vault's local snapshot index (0004 §2.5, 0007 §3; plan 0059).
//!
//! A local snapshot is a manifest the vault keeps after a commit replaces it,
//! together with every object that manifest references. The one
//! `snapshot_index` row holds a sealed blob listing the snapshots and the
//! sorted set of object IDs any of them references, so a commit can tell
//! which objects it may delete without decrypting every kept manifest. The
//! local head names the blob's digest (key 9), so the index cannot be swapped
//! for an older one without the head.
//!
//! Development wire format (volatile until the pre-1.0 freeze):
//!
//! ```text
//! row    = [1, data_profile uint, nonce bstr 24, ciphertext bstr]
//! sealed = AEAD(local-state key, aad, plaintext)
//! aad    = ["qv/snapshot-index/v1", vault_id, key_generation, data_profile]
//! plaintext = [1, rows, retained]
//! rows      = [[snapshot_id bstr 16, created_at int, kind uint (0 ordinary,
//!               1 recovery), manifest_hash bstr 32, reference_digest bstr 32]*]
//!             strictly ascending by snapshot ID, at most [`MAX_SNAPSHOTS`]
//! retained  = [object_id bstr 16 *], strictly ascending, at most
//!             [`MAX_RETAINED`]
//! ```
//!
//! The index alone proves nothing about the kept manifests and objects: a
//! restore reads them as an archive (0004 §6.3) and `verify` re-hashes them.

use qv_crypto::{ObjectHashProfile, object_hash};
use qv_encoding::{Decoder, Encoder, encode_array};
use qv_model::{ObjectId, RandomId, Timestamp};
use rusqlite::Connection;

use crate::{
    SnapshotKind, StoreError,
    header::Header,
    schema::validation_err,
    vault::{Keys, accepted_profile},
};

/// Most snapshots one vault keeps, whatever the policy asks (the default
/// policy keeps 48).
pub const MAX_SNAPSHOTS: usize = 256;

/// Most distinct object IDs the snapshots may keep between them.
pub const MAX_RETAINED: usize = 1 << 21;

/// Largest sealed index ciphertext: [`MAX_RETAINED`] IDs and every row, with
/// room for CBOR heads and padding.
pub const MAX_INDEX_CIPHERTEXT: usize = 48 * 1024 * 1024;

/// Largest stored row, checked before it is read.
pub const MAX_INDEX_ENVELOPE: usize = MAX_INDEX_CIPHERTEXT + 128;

const FORMAT: u64 = 1;
const LABEL: &str = "qv/snapshot-index/v1";

/// One kept snapshot. Not secret on its own, but stored sealed: the times and
/// count of snapshots reveal when the vault changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotRow {
    /// Random 128-bit snapshot ID.
    pub id: [u8; 16],
    /// When the commit that kept it ran.
    pub created_at: Timestamp,
    /// Why it was kept.
    pub kind: SnapshotKind,
    /// The kept manifest row.
    pub manifest_hash: [u8; 32],
    /// The kept manifest's reference digest (0004 §3.5): equal digests name
    /// the same stored objects, so a commit after an unchanged one keeps no
    /// second copy.
    pub reference_digest: [u8; 32],
}

impl SnapshotRow {
    /// The ID as 32 lowercase hex digits.
    pub fn id_hex(&self) -> String {
        self.id.iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// The decoded index: rows by ID and the retained object set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotIndex {
    rows: Vec<SnapshotRow>,
    retained: Vec<ObjectId>,
}

fn malformed<T>(_: T) -> StoreError {
    StoreError::Integrity
}

fn kind_code(kind: SnapshotKind) -> u64 {
    match kind {
        SnapshotKind::Ordinary => 0,
        SnapshotKind::Recovery => 1,
    }
}

fn read_len(d: &mut Decoder<'_>, max: usize) -> Result<usize, StoreError> {
    let n =
        usize::try_from(d.array().map_err(malformed)?).map_err(|_| StoreError::LimitExceeded)?;
    if n > max {
        return Err(StoreError::LimitExceeded);
    }
    Ok(n)
}

impl SnapshotIndex {
    /// The kept snapshots, ascending by ID.
    pub fn rows(&self) -> &[SnapshotRow] {
        &self.rows
    }

    /// The newest row by creation time, then ID.
    pub fn newest(&self) -> Option<&SnapshotRow> {
        self.rows
            .iter()
            .max_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)))
    }

    /// Whether any kept snapshot references `id`.
    pub fn retains(&self, id: ObjectId) -> bool {
        self.retained.binary_search(&id).is_ok()
    }

    /// The retained object IDs, ascending.
    pub(crate) fn retained(&self) -> &[ObjectId] {
        &self.retained
    }

    /// Whether a row names `hash`.
    pub(crate) fn names_manifest(&self, hash: &[u8; 32]) -> bool {
        self.rows.iter().any(|r| &r.manifest_hash == hash)
    }

    /// Adds a row; the ID must be new.
    pub(crate) fn insert(&mut self, row: SnapshotRow) -> Result<(), StoreError> {
        match self.rows.binary_search_by(|r| r.id.cmp(&row.id)) {
            Ok(_) => Err(StoreError::Internal),
            Err(_) if self.rows.len() >= MAX_SNAPSHOTS => Err(StoreError::LimitExceeded),
            Err(i) => {
                self.rows.insert(i, row);
                Ok(())
            }
        }
    }

    /// Removes a row by ID; `false` if absent.
    pub(crate) fn remove(&mut self, id: &[u8; 16]) -> bool {
        match self.rows.binary_search_by(|r| r.id.cmp(id)) {
            Ok(i) => {
                self.rows.remove(i);
                true
            }
            Err(_) => false,
        }
    }

    /// Replaces the retained set (sorted and deduplicated here).
    pub(crate) fn set_retained(&mut self, mut ids: Vec<ObjectId>) -> Result<(), StoreError> {
        ids.sort_unstable();
        ids.dedup();
        if ids.len() > MAX_RETAINED {
            return Err(StoreError::LimitExceeded);
        }
        self.retained = ids;
        Ok(())
    }

    /// The canonical plaintext.
    pub(crate) fn encode(&self) -> Result<qv_crypto::SecretBytes, StoreError> {
        let mut e = Encoder::new();
        e.array(|a| {
            a.uint(FORMAT);
            a.array(|rows| {
                for r in &self.rows {
                    rows.array(|x| {
                        x.bytes(&r.id);
                        x.int(r.created_at.0);
                        x.uint(kind_code(r.kind));
                        x.bytes(&r.manifest_hash);
                        x.bytes(&r.reference_digest);
                        Ok(())
                    })?;
                }
                Ok(())
            })?;
            a.array(|ids| {
                for id in &self.retained {
                    ids.bytes(&id.to_bytes());
                }
                Ok(())
            })
        })
        .map_err(|_| StoreError::Internal)?;
        let mut bytes = e.finish().map_err(|_| StoreError::Internal)?;
        Ok(qv_crypto::SecretBytes::take_from(&mut bytes))
    }

    /// Decodes authenticated plaintext with every bound and order rule.
    pub(crate) fn decode(plaintext: &[u8]) -> Result<Self, StoreError> {
        let mut d = Decoder::new(plaintext).map_err(malformed)?;
        if d.array().map_err(malformed)? != 3 {
            return Err(StoreError::Integrity);
        }
        if d.uint().map_err(malformed)? != FORMAT {
            return Err(StoreError::Unsupported);
        }
        let n = read_len(&mut d, MAX_SNAPSHOTS)?;
        let mut rows: Vec<SnapshotRow> = Vec::with_capacity(n);
        for _ in 0..n {
            if d.array().map_err(malformed)? != 5 {
                return Err(StoreError::Integrity);
            }
            let id: [u8; 16] = d
                .bytes()
                .map_err(malformed)?
                .try_into()
                .map_err(malformed)?;
            let created_at = Timestamp(d.int().map_err(malformed)?);
            let kind = match d.uint().map_err(malformed)? {
                0 => SnapshotKind::Ordinary,
                1 => SnapshotKind::Recovery,
                _ => return Err(StoreError::Integrity),
            };
            let manifest_hash = d
                .bytes()
                .map_err(malformed)?
                .try_into()
                .map_err(malformed)?;
            let reference_digest = d
                .bytes()
                .map_err(malformed)?
                .try_into()
                .map_err(malformed)?;
            if rows.last().is_some_and(|last| last.id >= id) {
                return Err(StoreError::Integrity);
            }
            rows.push(SnapshotRow {
                id,
                created_at,
                kind,
                manifest_hash,
                reference_digest,
            });
        }
        let n = read_len(&mut d, MAX_RETAINED)?;
        let mut retained: Vec<ObjectId> = Vec::with_capacity(n);
        for _ in 0..n {
            let raw: [u8; 16] = d
                .bytes()
                .map_err(malformed)?
                .try_into()
                .map_err(malformed)?;
            let id = ObjectId::from_bytes(raw).map_err(malformed)?;
            if retained.last().is_some_and(|last| *last >= id) {
                return Err(StoreError::Integrity);
            }
            retained.push(id);
        }
        d.finish().map_err(malformed)?;
        Ok(Self { rows, retained })
    }
}

fn aad(header: &Header, profile: u16) -> Result<zeroize::Zeroizing<Vec<u8>>, StoreError> {
    encode_array(|a| {
        a.text(LABEL);
        a.bytes(&header.vault_id.0);
        a.uint(u64::from(header.key_generation.0));
        a.uint(u64::from(profile));
        Ok(())
    })
    .map_err(|_| StoreError::Internal)
}

/// The digest the head names.
pub(crate) fn digest(blob: &[u8]) -> [u8; 32] {
    object_hash(ObjectHashProfile::V1, blob).0
}

/// Seals `index` into a stored row.
pub(crate) fn seal(
    index: &SnapshotIndex,
    keys: &Keys,
    header: &Header,
) -> Result<Vec<u8>, StoreError> {
    let plaintext = index.encode()?;
    let sealed = qv_crypto::seal(
        &keys.local,
        header.data_profile,
        &aad(header, header.data_profile.id())?,
        plaintext.expose_secret(),
        MAX_INDEX_CIPHERTEXT,
    )
    .map_err(|e| match e {
        qv_crypto::CryptoError::LimitExceeded => StoreError::LimitExceeded,
        _ => StoreError::Internal,
    })?;
    Ok(encode_array(|a| {
        a.uint(FORMAT);
        a.uint(u64::from(header.data_profile.id()));
        a.bytes(&sealed.nonce);
        a.bytes(&sealed.ciphertext);
        Ok(())
    })
    .map_err(|_| StoreError::Internal)?
    .to_vec())
}

/// Opens a stored row that must match the head's `expected` digest.
pub(crate) fn open(
    blob: &[u8],
    expected: &[u8; 32],
    keys: &Keys,
    header: &Header,
) -> Result<SnapshotIndex, StoreError> {
    if blob.len() > MAX_INDEX_ENVELOPE {
        return Err(StoreError::LimitExceeded);
    }
    if &digest(blob) != expected {
        return Err(StoreError::Integrity);
    }
    let mut d = Decoder::new(blob).map_err(malformed)?;
    if d.array().map_err(malformed)? != 4 {
        return Err(StoreError::Integrity);
    }
    if d.uint().map_err(malformed)? != FORMAT {
        return Err(StoreError::Unsupported);
    }
    let profile = u16::try_from(d.uint().map_err(malformed)?).map_err(malformed)?;
    let profile = accepted_profile(header, profile)?;
    let nonce = d
        .bytes()
        .map_err(malformed)?
        .try_into()
        .map_err(malformed)?;
    let ciphertext = d.bytes().map_err(malformed)?;
    d.finish().map_err(malformed)?;
    let plaintext = qv_crypto::open(
        &keys.local,
        profile,
        &aad(header, profile.id())?,
        &nonce,
        ciphertext,
        MAX_INDEX_CIPHERTEXT,
    )
    .map_err(|e| match e {
        qv_crypto::CryptoError::LimitExceeded => StoreError::LimitExceeded,
        _ => StoreError::Integrity,
    })?;
    SnapshotIndex::decode(plaintext.expose_secret())
}

/// Checks the row's count, type and length, then reads it.
pub(crate) fn read(conn: &Connection) -> Result<Vec<u8>, StoreError> {
    check(conn)?;
    conn.query_row("SELECT sealed FROM snapshot_index WHERE id = 1", [], |r| {
        r.get(0)
    })
    .map_err(|e| validation_err(&e))
}

/// Checks the row's count, type and length without reading it.
pub(crate) fn check(conn: &Connection) -> Result<(), StoreError> {
    let v = |e: rusqlite::Error| validation_err(&e);
    let count: i64 = conn
        .query_row("SELECT count(*) FROM snapshot_index", [], |r| r.get(0))
        .map_err(v)?;
    if count != 1 {
        return Err(StoreError::Integrity);
    }
    let (id, kind, len): (i64, String, i64) = conn
        .query_row(
            "SELECT id, typeof(sealed), octet_length(sealed) FROM snapshot_index",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(v)?;
    if id != 1 || kind != "blob" || len < 0 {
        return Err(StoreError::Integrity);
    }
    if usize::try_from(len).map_err(malformed)? > MAX_INDEX_ENVELOPE {
        return Err(StoreError::LimitExceeded);
    }
    Ok(())
}

/// Stores `blob` as the one row, creating it on the first write of an
/// upgraded file. Inside the caller's write transaction.
pub(crate) fn store(conn: &Connection, blob: &[u8], fresh: bool) -> Result<(), rusqlite::Error> {
    if fresh {
        conn.execute(
            "INSERT INTO snapshot_index (id, sealed) VALUES (1, ?1)",
            [blob],
        )?;
    } else {
        conn.execute("UPDATE snapshot_index SET sealed = ?1 WHERE id = 1", [blob])?;
    }
    Ok(())
}

/// Only new-file builders initialize the empty index.
pub(crate) fn initialize(
    conn: &Connection,
    keys: &Keys,
    header: &Header,
) -> Result<[u8; 32], StoreError> {
    let blob = seal(&SnapshotIndex::default(), keys, header)?;
    store(conn, &blob, true).map_err(|e| validation_err(&e))?;
    Ok(digest(&blob))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn row(n: u8, at: i64) -> SnapshotRow {
        SnapshotRow {
            id: [n; 16],
            created_at: Timestamp(at),
            kind: if n.is_multiple_of(2) {
                SnapshotKind::Ordinary
            } else {
                SnapshotKind::Recovery
            },
            manifest_hash: [n; 32],
            reference_digest: [n.wrapping_add(1); 32],
        }
    }

    fn oid(n: u8) -> ObjectId {
        let mut b = [0u8; 16];
        b[0] = n;
        b[6] = 0x40;
        b[8] = 0x80;
        ObjectId::from_bytes(b).unwrap()
    }

    #[test]
    fn snapshot_index_round_trips_and_refuses_disorder_and_bounds() {
        let mut index = SnapshotIndex::default();
        index.insert(row(2, 20)).unwrap();
        index.insert(row(1, 30)).unwrap();
        assert_eq!(index.insert(row(1, 40)).err(), Some(StoreError::Internal));
        index.set_retained(vec![oid(3), oid(1), oid(3)]).unwrap();
        assert!(index.retains(oid(1)) && !index.retains(oid(2)));
        assert_eq!(index.newest().unwrap().id, [1; 16]);
        assert!(index.names_manifest(&[2; 32]));
        let bytes = index.encode().unwrap();
        assert_eq!(SnapshotIndex::decode(bytes.expose_secret()).unwrap(), index);
        assert!(index.remove(&[2; 16]) && !index.remove(&[2; 16]));

        // Rows out of order and an unknown kind are refused.
        let raw = |rows: &[([u8; 16], u64)], retained: &[ObjectId]| {
            let mut e = Encoder::new();
            e.array(|a| {
                a.uint(FORMAT);
                a.array(|x| {
                    for (id, kind) in rows {
                        x.array(|r| {
                            r.bytes(id);
                            r.int(1);
                            r.uint(*kind);
                            r.bytes(&[0; 32]);
                            r.bytes(&[0; 32]);
                            Ok(())
                        })?;
                    }
                    Ok(())
                })?;
                a.array(|x| {
                    for id in retained {
                        x.bytes(&id.to_bytes());
                    }
                    Ok(())
                })
            })
            .unwrap();
            e.finish().unwrap()
        };
        assert!(SnapshotIndex::decode(&raw(&[([1; 16], 0), ([2; 16], 1)], &[oid(1)])).is_ok());
        for bad in [
            raw(&[([2; 16], 0), ([1; 16], 0)], &[]),
            raw(&[([1; 16], 0), ([1; 16], 0)], &[]),
            raw(&[([1; 16], 2)], &[]),
            raw(&[], &[oid(2), oid(1)]),
        ] {
            assert_eq!(
                SnapshotIndex::decode(&bad).err(),
                Some(StoreError::Integrity)
            );
        }
        let too_many: Vec<_> = (0..=MAX_SNAPSHOTS as u32)
            .map(|i| {
                let mut id = [0u8; 16];
                id[..4].copy_from_slice(&i.to_be_bytes());
                (id, 0)
            })
            .collect();
        assert_eq!(
            SnapshotIndex::decode(&raw(&too_many, &[])).err(),
            Some(StoreError::LimitExceeded)
        );
    }
}
