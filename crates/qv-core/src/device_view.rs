// SPDX-License-Identifier: AGPL-3.0-or-later

//! Device view notes (plan 0063): a little per-device UI state, such as the
//! last opened group, that the app keeps in its own data directory. Each note
//! is sealed under the vault's `device-view` purpose key (0002 §3.3) and
//! filed under a tag MACed with that key, so without VK the app's file holds
//! no readable vault data and cannot be linked to a vault file. A new key
//! generation (rotation) makes old notes unreadable, which is harmless.
use qv_crypto::{KeyGeneration, MacProfile, SecretBytes, StoredDataProfile, VaultId, purpose};
use zeroize::Zeroizing;

use crate::error::{CoreError, crypto};
use crate::session::Session;

/// The longest note (a group's 32-hex record ID fits).
pub const DEVICE_VIEW_NOTE_MAX: usize = 64;
/// The longest sealed note: the 24-byte nonce, then the ciphertext.
pub const DEVICE_VIEW_SEALED_MAX: usize = 256;
const NONCE: usize = 24;
/// Every note is sealed at this plaintext length (a length byte, the note,
/// zeros), so the sealed size does not tell an empty note from a group.
const PADDED: usize = 1 + DEVICE_VIEW_NOTE_MAX;

/// The key and the scope its AAD and tag bind (0002 §7.2).
struct ViewKey {
    key: qv_crypto::PurposeKey<purpose::DeviceView>,
    vault_id: VaultId,
    generation: KeyGeneration,
}

impl ViewKey {
    /// `["qv/device-view/tag/v1", vault_id, key_generation]`.
    fn tag_message(&self) -> Result<Zeroizing<Vec<u8>>, CoreError> {
        qv_encoding::encode_array(|a| {
            a.text("qv/device-view/tag/v1");
            a.bytes(&self.vault_id.0);
            a.uint(u64::from(self.generation.0));
            Ok(())
        })
        .map_err(|_| CoreError::Internal)
    }

    /// `["qv/device-view/note/v1", vault_id, key_generation, data_profile]`.
    fn note_aad(&self, profile: StoredDataProfile) -> Result<Zeroizing<Vec<u8>>, CoreError> {
        qv_encoding::encode_array(|a| {
            a.text("qv/device-view/note/v1");
            a.bytes(&self.vault_id.0);
            a.uint(u64::from(self.generation.0));
            a.uint(u64::from(profile.id()));
            Ok(())
        })
        .map_err(|_| CoreError::Internal)
    }
}

impl Session {
    fn device_view_key(&self) -> Result<ViewKey, CoreError> {
        self.read(|ctx| {
            let h = ctx.open.vault.header();
            let key = ctx
                .open
                .vault
                .vault_key()
                .purpose_key::<purpose::DeviceView>(
                    h.vault_id,
                    h.key_generation,
                    h.derivation_profile,
                )
                .map_err(crypto)?;
            Ok(ViewKey {
                key,
                vault_id: h.vault_id,
                generation: h.key_generation,
            })
        })
    }

    /// This vault's note name on this device: 64 hex digits, stable for the
    /// vault and its key generation, meaningless without VK.
    pub fn device_view_tag(&self) -> Result<String, CoreError> {
        let k = self.device_view_key()?;
        let tag = qv_crypto::mac(&k.key, MacProfile::V1, &k.tag_message()?).map_err(crypto)?;
        Ok(crate::setup::hex(tag.as_bytes()))
    }

    /// Seals `note` (at most [`DEVICE_VIEW_NOTE_MAX`] bytes) for this vault.
    pub fn seal_device_view(&self, note: &[u8]) -> Result<Vec<u8>, CoreError> {
        let len = u8::try_from(note.len())
            .ok()
            .filter(|_| note.len() <= DEVICE_VIEW_NOTE_MAX)
            .ok_or(CoreError::LimitExceeded)?;
        let mut padded = Zeroizing::new([0u8; PADDED]);
        padded[0] = len;
        padded[1..=note.len()].copy_from_slice(note);
        let k = self.device_view_key()?;
        let profile = StoredDataProfile::V1;
        let sealed = qv_crypto::seal(
            &k.key,
            profile,
            &k.note_aad(profile)?,
            padded.as_slice(),
            DEVICE_VIEW_SEALED_MAX - NONCE,
        )
        .map_err(crypto)?;
        let mut out = Vec::with_capacity(NONCE + sealed.ciphertext.len());
        out.extend_from_slice(&sealed.nonce);
        out.extend_from_slice(&sealed.ciphertext);
        Ok(out)
    }

    /// Opens a note sealed for this vault and key generation. Anything else,
    /// including a note of another vault or generation, is
    /// [`CoreError::NotFound`].
    pub fn open_device_view(&self, sealed: &[u8]) -> Result<SecretBytes, CoreError> {
        if sealed.len() > DEVICE_VIEW_SEALED_MAX {
            return Err(CoreError::LimitExceeded);
        }
        let (Some(nonce), Some(ciphertext)) = (sealed.get(..NONCE), sealed.get(NONCE..)) else {
            return Err(CoreError::NotFound);
        };
        let nonce: &[u8; NONCE] = nonce.try_into().map_err(|_| CoreError::NotFound)?;
        let k = self.device_view_key()?;
        let profile = StoredDataProfile::V1;
        let padded = qv_crypto::open(
            &k.key,
            profile,
            &k.note_aad(profile)?,
            nonce,
            ciphertext,
            DEVICE_VIEW_SEALED_MAX - NONCE,
        )
        .map_err(|_| CoreError::NotFound)?;
        let padded = padded.expose_secret();
        let len = usize::from(*padded.first().ok_or(CoreError::NotFound)?);
        if padded.len() != PADDED || len > DEVICE_VIEW_NOTE_MAX {
            return Err(CoreError::NotFound);
        }
        SecretBytes::try_init(len, |out| {
            out.copy_from_slice(&padded[1..=len]);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::Env;

    #[test]
    fn device_view_notes_round_trip_and_bind_to_their_vault() {
        let a = Env::new();
        let s = a.unlock_with(None);
        let tag = s.device_view_tag().unwrap();
        assert_eq!(tag.len(), 64);
        assert_eq!(tag, s.device_view_tag().unwrap(), "stable");
        let sealed = s
            .seal_device_view(b"0123456789abcdef0123456789abcdef")
            .unwrap();
        assert!(sealed.len() <= super::DEVICE_VIEW_SEALED_MAX);
        assert!(
            !sealed
                .windows(32)
                .any(|w| w == b"0123456789abcdef0123456789abcdef"),
            "sealed, not stored in clear"
        );
        assert_eq!(
            s.open_device_view(&sealed).unwrap().expose_secret(),
            b"0123456789abcdef0123456789abcdef"
        );
        // Fresh nonces: the same note seals differently.
        assert_ne!(
            sealed,
            s.seal_device_view(b"0123456789abcdef0123456789abcdef")
                .unwrap()
        );
        // An empty note means "All items"; it seals to the same size as a
        // full one.
        let empty = s.seal_device_view(b"").unwrap();
        assert_eq!(empty.len(), sealed.len());
        assert!(
            s.open_device_view(&empty)
                .unwrap()
                .expose_secret()
                .is_empty()
        );
        // Tampering and wrong sizes refuse.
        let mut bad = sealed.clone();
        *bad.last_mut().unwrap() ^= 1;
        assert_eq!(
            s.open_device_view(&bad).unwrap_err(),
            crate::CoreError::NotFound
        );
        assert_eq!(
            s.open_device_view(&[]).unwrap_err(),
            crate::CoreError::NotFound
        );
        assert_eq!(
            s.open_device_view(&[0; super::DEVICE_VIEW_SEALED_MAX + 1])
                .unwrap_err(),
            crate::CoreError::LimitExceeded
        );
        assert_eq!(
            s.seal_device_view(&[b'a'; super::DEVICE_VIEW_NOTE_MAX + 1])
                .unwrap_err(),
            crate::CoreError::LimitExceeded
        );
        // Binding to the vault, its key generation and VK is the purpose
        // key's: qv-crypto `keys_depend_on_vault_generation_profile_and_vk`
        // covers `device-view` (test fixtures share one vault and VK).
    }
}
