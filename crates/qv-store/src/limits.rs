// SPDX-License-Identifier: AGPL-3.0-or-later

//! Vault-file budgets (spec 0004 §4.1), frozen at 1.0 (spec 0015 §1.5).

use crate::object::ENVELOPE_OVERHEAD;

/// Largest vault file accepted before SQLite parses it: the 8 GiB
/// referenced-ciphertext budget plus room for history, snapshot-only
/// objects and retained manifests (plan 0059).
pub const MAX_DB_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// The only accepted SQLite page size.
pub const PAGE_SIZE: u64 = 4096;

/// Most bytes of header and local keyslot metadata together (0004 §4.1).
pub const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Most local keyslots (0004 §4.1).
pub const MAX_SLOTS: usize = 64;

/// Largest padded record ciphertext (0004 §4.1).
pub const MAX_RECORD_CIPHERTEXT: usize = 1024 * 1024;

/// Largest manifest ciphertext (0004 §4.1).
pub const MAX_MANIFEST_CIPHERTEXT: usize = 64 * 1024 * 1024;

/// Most record IDs in a manifest, tombstones included (0004 §4.1).
pub const MAX_RECORDS: usize = 50_000;

/// Most referenced objects in a manifest other than attachment chunks: the
/// record versions (0004 §4.1).
pub const MAX_OBJECTS: usize = 250_000;

/// Most attachment chunks a manifest references: the total budget in 64 KiB
/// chunks (0004 §4.1, plan 0059). Counted apart from [`MAX_OBJECTS`].
pub const MAX_CHUNK_OBJECTS: usize = 131_072;

/// Most references of any kind in a manifest.
pub const MAX_REFERENCES: usize = MAX_OBJECTS + MAX_CHUNK_OBJECTS;

/// Most referenced ciphertext bytes in total (0004 §4.1, plan 0059).
pub const MAX_TOTAL_CIPHERTEXT: u64 = 8 * 1024 * 1024 * 1024;

/// Most clock components across a manifest's heads, history and floors
/// (0004 §4.1, amended by plan 0009).
pub const MAX_CLOCK_COMPONENTS: usize = 1 << 20;

/// Most chunks of one attachment: 2 GiB in 64 KiB chunks (0003 §6).
pub const MAX_CHUNKS: usize = (qv_model::MAX_ATTACHMENT_BYTES / qv_model::CHUNK_SIZE) as usize;

/// Largest object frame of an archive: one record or chunk envelope at its
/// ciphertext budget (0004 §4.1, §6.1).
pub const MAX_ARCHIVE_OBJECT_FRAME: usize = MAX_RECORD_CIPHERTEXT + ENVELOPE_OVERHEAD;

/// Largest manifest frame of an archive: a manifest envelope at the manifest
/// ciphertext budget (0004 §4.1, §6.3).
pub const MAX_ARCHIVE_MANIFEST_FRAME: usize = MAX_MANIFEST_CIPHERTEXT + ENVELOPE_OVERHEAD;

/// Largest sum of referenced ciphertext lengths one archive may claim: the
/// vault's own total budget, so a header is refused before any frame is read
/// (0004 §4.1).
pub const MAX_ARCHIVE_TOTAL_LENGTH: u64 = MAX_TOTAL_CIPHERTEXT;
