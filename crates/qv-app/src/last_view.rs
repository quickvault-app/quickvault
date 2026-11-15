// SPDX-License-Identifier: AGPL-3.0-or-later

//! The group each vault last showed, kept across restarts (0009 §1.8, plan
//! 0063). A `views` file beside `preferences` holds at most 16 lines
//! `<tag>=<sealed>` (hex), most recent first. The tag and the sealed note
//! come from the vault's `device-view` key (`qv_core::Session`), so without
//! VK the file names no vault and holds no readable vault data. The note is
//! the group's record ID, or empty for "All items". Bounded, parsed
//! strictly, never read through a link, written 0600 and renamed into
//! place; every failure falls back to "All items".
use std::path::Path;

use crate::prefs::{read_private, write_private};

const FILE: &str = "views";
const TEMP: &str = "views.tmp";
/// The most vaults remembered; the least recently used is dropped.
pub const MAX_VIEWS: usize = 16;
const TAG_HEX: usize = 64;
const SEALED_HEX_MAX: usize = 2 * qv_core::DEVICE_VIEW_SEALED_MAX;
/// The largest file read: every line at its longest.
const MAX_FILE: usize = MAX_VIEWS * (TAG_HEX + 1 + SEALED_HEX_MAX + 1);

/// The remembered views: (tag, sealed note), most recent first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Views(Vec<(String, Vec<u8>)>);

fn lower_hex(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !lower_hex(s) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

impl Views {
    /// Parse bounded bytes; malformed and repeated lines are ignored.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut out = Vec::new();
        if bytes.len() > MAX_FILE {
            return Self(out);
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Self(out);
        };
        for line in text.lines() {
            let Some((tag, sealed)) = line.split_once('=') else {
                continue;
            };
            if tag.len() != TAG_HEX
                || !lower_hex(tag)
                || sealed.is_empty()
                || sealed.len() > SEALED_HEX_MAX
                || out.iter().any(|(t, _)| t == tag)
            {
                continue;
            }
            if let Some(bytes) = unhex(sealed) {
                out.push((tag.to_owned(), bytes));
            }
            if out.len() == MAX_VIEWS {
                break;
            }
        }
        Self(out)
    }
    /// The file contents.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        for (tag, sealed) in &self.0 {
            out.push_str(tag);
            out.push('=');
            for b in sealed {
                out.push_str(&format!("{b:02x}"));
            }
            out.push('\n');
        }
        out
    }
    /// The sealed note filed under `tag`.
    pub fn get(&self, tag: &str) -> Option<&[u8]> {
        self.0
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, s)| s.as_slice())
    }
    /// Files `sealed` under `tag` as the most recent view.
    pub fn put(&mut self, tag: &str, sealed: Vec<u8>) {
        if tag.len() != TAG_HEX || !lower_hex(tag) || sealed.len() * 2 > SEALED_HEX_MAX {
            return;
        }
        self.0.retain(|(t, _)| t != tag);
        self.0.insert(0, (tag.to_owned(), sealed));
        self.0.truncate(MAX_VIEWS);
    }
    /// Load from the private app directory; any problem gives none.
    pub fn load(base: &Path) -> Self {
        read_private(&base.join(FILE), MAX_FILE).map_or_else(Self::default, |b| Self::parse(&b))
    }
    /// Save atomically: a fresh 0600 temporary file renamed into place.
    pub fn save(&self, base: &Path) -> std::io::Result<()> {
        write_private(base, FILE, TEMP, self.encode().as_bytes())
    }
}

/// The group to open for this vault, from its sealed note: a live group, or
/// `None` ("All items") for anything else.
pub(crate) fn restore(
    session: &qv_core::Session,
    views: &Views,
) -> Option<(String, qv_core::GroupHandle)> {
    let tag = session.device_view_tag().ok()?;
    let note = session.open_device_view(views.get(&tag)?).ok()?;
    let id = std::str::from_utf8(note.expose_secret()).ok()?;
    if id.len() != 32 || !lower_hex(id) {
        return None;
    }
    match session.find_id(id, false).ok()?.as_slice() {
        [qv_core::ItemHandle::Group(g)] if g.id_hex() == id => Some((id.to_owned(), *g)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    #[test]
    fn last_view_file_round_trips_and_keeps_the_most_recent() {
        let mut v = Views::default();
        v.put(&tag(1), vec![1, 2, 3]);
        v.put(&tag(2), vec![4]);
        v.put(&tag(1), vec![9]);
        assert_eq!(v.get(&tag(1)), Some(&[9][..]));
        let text = v.encode();
        assert_eq!(text, format!("{}=09\n{}=04\n", tag(1), tag(2)));
        assert_eq!(Views::parse(text.as_bytes()), v);
        for n in 0..40 {
            v.put(&tag(n), vec![n]);
        }
        assert_eq!(v.0.len(), MAX_VIEWS);
        assert_eq!(v.0[0].0, tag(39), "most recent first");
        assert!(v.get(&tag(1)).is_none(), "the oldest went");
    }

    #[test]
    fn last_view_file_ignores_malformed_lines() {
        let good = format!("{}=0a0b\n", tag(7));
        let text = format!(
            "junk\n{}=0a0b\n{}=zz\n{}=abc\n{}=\nAB{}=00\n{}=ff\n{good}",
            "0".repeat(63),
            tag(3),
            tag(4),
            tag(5),
            "c".repeat(62),
            tag(7),
        );
        let v = Views::parse(text.as_bytes());
        // Only the first line for tag 7 counts; the rest are refused.
        assert_eq!(v.0, vec![(tag(7), vec![0xff])]);
        assert_eq!(Views::parse(&vec![b'a'; MAX_FILE + 1]), Views::default());
        assert_eq!(Views::parse(&[0xff, 0xfe]), Views::default());
        // Bad tags and oversized notes are never filed.
        let mut v = Views::default();
        v.put("short", vec![1]);
        v.put(&tag(0xab).to_uppercase(), vec![1]);
        v.put(&tag(1), vec![0; qv_core::DEVICE_VIEW_SEALED_MAX + 1]);
        assert_eq!(v, Views::default());
    }
}
