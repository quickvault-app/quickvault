// SPDX-License-Identifier: AGPL-3.0-or-later

//! Device view notes (plan 0063): a little per-device UI state, such as the
//! last opened group, that the app keeps in its own data directory. Each note
//! is sealed under the vault's `device-view` purpose key (0002 §3.3) and
//! filed under a tag MACed with that key, so without VK the app's file holds
//! no readable vault data and cannot be linked to a vault file. A new key
//! generation (rotation) makes old notes unreadable, which is harmless.
use qv_crypto::{MacProfile, SecretBytes, StoredDataProfile, purpose};

use crate::error::{CoreError, crypto};
use crate::session::Session;

/// The longest note (a group's 32-hex record ID fits).
pub const DEVICE_VIEW_NOTE_MAX: usize = 64;
/// The longest sealed note: the 24-byte nonce, then the ciphertext.
pub const DEVICE_VIEW_SEALED_MAX: usize = 256;
const NONCE: usize = 24;
const TAG_MESSAGE: &[u8] = b"qv/device-view/tag/v1";
const NOTE_AAD: &[u8] = b"qv/device-view/note/v1";

impl Session {
    fn device_view_key(&self) -> Result<qv_crypto::PurposeKey<purpose::DeviceView>, CoreError> {
        self.read(|ctx| {
            let h = ctx.open.vault.header();
            ctx.open
                .vault
                .vault_key()
                .purpose_key::<purpose::DeviceView>(
                    h.vault_id,
                    h.key_generation,
                    h.derivation_profile,
                )
                .map_err(crypto)
        })
    }

    /// This vault's note name on this device: 64 hex digits, stable for the
    /// vault and its key generation, meaningless without VK.
    pub fn device_view_tag(&self) -> Result<String, CoreError> {
        let key = self.device_view_key()?;
        let tag = qv_crypto::mac(&key, MacProfile::V1, TAG_MESSAGE).map_err(crypto)?;
        Ok(crate::setup::hex(tag.as_bytes()))
    }

    /// Seals `note` (at most [`DEVICE_VIEW_NOTE_MAX`] bytes) for this vault.
    pub fn seal_device_view(&self, note: &[u8]) -> Result<Vec<u8>, CoreError> {
        if note.len() > DEVICE_VIEW_NOTE_MAX {
            return Err(CoreError::LimitExceeded);
        }
        let key = self.device_view_key()?;
        let sealed = qv_crypto::seal(
            &key,
            StoredDataProfile::V1,
            NOTE_AAD,
            note,
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
        let key = self.device_view_key()?;
        let note = qv_crypto::open(
            &key,
            StoredDataProfile::V1,
            NOTE_AAD,
            nonce,
            ciphertext,
            DEVICE_VIEW_SEALED_MAX - NONCE,
        )
        .map_err(|_| CoreError::NotFound)?;
        if note.len() > DEVICE_VIEW_NOTE_MAX {
            return Err(CoreError::NotFound);
        }
        Ok(note)
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
        // An empty note means "All items".
        let empty = s.seal_device_view(b"").unwrap();
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
