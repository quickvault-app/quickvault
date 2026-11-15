// SPDX-License-Identifier: AGPL-3.0-or-later

//! `qv snapshot ls`, snapshots kept inside commits and what `trash empty`
//! does to them (0011 §4–5, 0007 §3; plan 0059).
#![cfg(any(target_os = "linux", target_os = "android"))]
#![allow(clippy::unwrap_used, clippy::panic)]
mod common;

use std::os::unix::fs::PermissionsExt;

use common::{Home, Output, PASSWORD, arg, piped};
use qv_core::{ItemHandle, Session};

fn run(h: &Home, extra: &[&str]) -> Output {
    let vault = arg(&h.vault());
    let mut a = vec!["--password-stdin", "-v", &vault];
    a.extend_from_slice(extra);
    piped(h.qv(a), format!("{PASSWORD}\n").as_bytes())
}

fn trashed_entry(s: &Session) -> ItemHandle {
    let root = s.root().unwrap();
    let e = s.new_entry(root, "login", "Mail").unwrap();
    let e = ItemHandle::Entry(s.save_entry(e).unwrap());
    s.delete(e, false).unwrap();
    e
}

/// A change made with the CLI's snapshot policy: its commit keeps the state
/// before it inside the vault, at most once an hour (plan 0059).
fn change(h: &Home, name: &str) {
    let o = run(h, &["mkdir", name]);
    assert_eq!(o.code, 0, "{}", o.stderr);
}

#[test]
fn snapshot_ls_lists_authenticated_local_snapshots() {
    let h = Home::new();
    let s = h.create();
    drop(s);
    // A read keeps nothing; the first change keeps one ordinary snapshot.
    let o = run(&h, &["snapshot", "ls"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.is_empty(), "{}", o.stdout);
    change(&h, "First");
    let o = run(&h, &["snapshot", "ls"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "{}", o.stdout);
    let row = lines[0];
    let id = &row[..32];
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{row}"
    );
    assert!(row.contains("ordinary"), "{row}");
    assert!(row.contains(" bytes kept "), "{row}");
    assert!(row.contains(" objects  "), "{row}");

    let o = run(&h, &["--json", "snapshot", "ls"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let json = o.stdout.trim_end();
    assert!(json.starts_with("{\"snapshots\":[{\"id\":\""), "{json}");
    assert!(json.contains("\"created_at\":\""), "{json}");
    assert!(json.contains("\"kind\":\"ordinary\""), "{json}");
    assert!(json.contains("\"size\":"), "{json}");
    assert!(json.contains("\"objects\":"), "{json}");
    assert!(json.contains("\"total_bytes\":"), "{json}");
    assert!(json.ends_with("}]}"), "{json}");
    assert!(!json.contains(&h.root.to_string_lossy().to_string()));

    // A second change inside the hour coalesces: still one, and no file.
    change(&h, "Second");
    let o = run(&h, &["snapshot", "ls"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout.lines().count(), 1, "{}", o.stdout);
    assert_eq!(std::fs::read_dir(h.snapshots()).unwrap().count(), 0);
}

// 0012 §1: Hostile files from sync folders
#[test]
fn hostile_files_from_sync_folders_are_warnings_not_rows() {
    // 0012 §1 row "Hostile files from sync folders": an unauthenticatable
    // archive in the snapshot directory never becomes a row, never stops the
    // healthy ones being listed and never reaches the vault.
    let h = Home::new();
    let s = h.create();
    drop(s);
    change(&h, "Kept");
    let o = run(&h, &["snapshot", "ls"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let healthy = o.stdout.lines().count();
    assert_eq!(healthy, 1, "{}", o.stdout);
    let dir = h.snapshots();
    // A valid snapshot name with a truncated prefix and garbage frames.
    let hostile = "0123456789abcdef0123456789abcdef.qvsnap";
    std::fs::write(
        dir.join(hostile),
        b"QVSNAP\0\x01\x00\x00\x02\x00not a header",
    )
    .unwrap();
    let o = run(&h, &["snapshot", "ls"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout.lines().count(), healthy, "{}", o.stdout);
    assert!(!o.stdout.contains("0123456789abcdef"));
    assert!(
        o.stderr.contains("could not be authenticated"),
        "{}",
        o.stderr
    );
    // The vault still verifies, so no exchange byte reached SQLite.
    let o = run(&h, &["verify"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
}

#[test]
fn trash_empty_takes_no_copy_and_removes_the_snapshots_holding_the_items() {
    for keep in [false, true] {
        let h = Home::new();
        let s = h.create();
        let item = trashed_entry(&s);
        drop(s);
        // This change keeps the state holding the trashed entry.
        change(&h, "Other");
        let seq = h.unlock().overview().unwrap().local_seq;
        let mut args = vec!["--yes", "trash", "empty"];
        if keep {
            args.push("--keep-snapshots");
        }
        let o = run(&h, &args);
        assert_eq!(o.code, 0, "{}", o.stderr);
        assert!(o.stdout.is_empty(), "{}", o.stdout);
        let s = h.unlock();
        assert!(s.trash().unwrap().is_empty());
        assert_eq!(s.overview().unwrap().local_seq, seq + 1, "one purge commit");
        assert!(s.find_id(&item.id_hex(), true).unwrap().is_empty());
        drop(s);
        // Without --keep-snapshots the snapshot holding it went with it; no
        // recovery file was written either way.
        let o = run(&h, &["snapshot", "ls"]);
        assert_eq!(o.code, 0, "{}", o.stderr);
        assert_eq!(o.stdout.lines().count(), usize::from(keep), "{}", o.stdout);
        assert_eq!(std::fs::read_dir(h.snapshots()).unwrap().count(), 0);
    }
}

// 0012 §1: Snapshot disk growth
#[test]
fn snapshot_disk_growth_deletion_needs_no_snapshot_directory() {
    // 0012 §1 row "Snapshot disk growth": deletion takes no recovery copy
    // (plan 0059), so an unusable snapshot directory only disables
    // snapshots; the deletion itself still commits.
    let h = Home::new();
    let s = h.create();
    trashed_entry(&s);
    let seq = s.overview().unwrap().local_seq;
    drop(s);
    let dir = h.snapshots();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
    let o = run(&h, &["--yes", "trash", "empty"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(
        o.stderr.contains("local snapshots are disabled"),
        "{}",
        o.stderr
    );
    assert!(o.stdout.is_empty());
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let s = h.unlock();
    assert!(s.trash().unwrap().is_empty());
    assert_eq!(s.overview().unwrap().local_seq, seq + 1);
}

#[test]
fn snapshot_restore_creates_a_new_baseline_and_preserves_active_vault() {
    let h = Home::new();
    let s = h.create();
    let root = s.root().unwrap();
    s.save_entry(s.new_entry(root, "login", "Snapshot entry").unwrap())
        .unwrap();
    drop(s);
    change(&h, "After the snapshot");
    let listed = run(&h, &["snapshot", "ls"]);
    assert_eq!(listed.code, 0, "{}", listed.stderr);
    let id = listed.stdout.lines().next().unwrap()[..32].to_owned();
    let before = std::fs::read(h.vault()).unwrap();
    let anchors: Vec<_> = std::fs::read_dir(h.anchors())
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (p.clone(), std::fs::read(p).unwrap())
        })
        .collect();
    let target = h.vaults().join("restored.qvault");
    let path = arg(&target);
    let o = run(
        &h,
        &["snapshot", "restore", &format!("@{}", &id[..6]), &path],
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.is_empty());
    assert!(o.stderr.contains("new recovery baseline") && o.stderr.contains("may be old"));
    assert_eq!(std::fs::read(h.vault()).unwrap(), before);
    for (p, bytes) in anchors {
        assert_eq!(std::fs::read(p).unwrap(), bytes);
    }
    let o = piped(
        h.qv(["--password-stdin", "-v", &path, "--json", "verify"]),
        format!("{PASSWORD}\n").as_bytes(),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("\"local_seq\":1") && o.stdout.contains("\"anchor\":\"verified\""));
    let o = piped(
        h.qv(["--password-stdin", "-v", &path, "search", "Snapshot entry"]),
        format!("{PASSWORD}\n").as_bytes(),
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("Snapshot entry"));
    let target_before = std::fs::read(&target).unwrap();
    let o = run(&h, &["snapshot", "restore", &id, &path]);
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert_eq!(std::fs::read(&target).unwrap(), target_before);
}

#[test]
fn snapshot_restore_preflight_and_addressing_errors() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::new();
    let s = h.create();
    drop(s);
    for args in [
        vec!["--json", "snapshot", "restore", "abcdef", "new.qvault"],
        vec!["snapshot", "restore", "invalid", "new.qvault"],
    ] {
        let mut a = vec!["--password-stdin"];
        a.extend(args);
        let o = piped(h.qv(a), b"");
        assert_eq!(o.code, 2, "{}", o.stderr);
    }
    assert_eq!(run(&h, &["snapshot", "ls"]).code, 0);
    let target = arg(&h.vaults().join("new.qvault"));
    let o = run(&h, &["snapshot", "restore", &"f".repeat(32), &target]);
    assert_eq!(o.code, 4, "{}", o.stderr);
    let ids = [
        "abcdef00000000000000000000000000",
        "abcdef11111111111111111111111111",
    ];
    for id in ids {
        let path = h.snapshots().join(format!("{id}.qvsnap"));
        std::fs::write(&path, b"hostile archive").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let o = run(&h, &["snapshot", "restore", "abcdef", &target]);
    assert_eq!(o.code, 5, "{}", o.stderr);
    for id in ids {
        assert!(o.stderr.contains(id));
    }
    let o = run(&h, &["snapshot", "restore", ids[0], &target]);
    assert_eq!(o.code, 7, "{}", o.stderr);
    assert!(!h.vaults().join("new.qvault").exists());
}

#[test]
fn snapshot_restore_slot_bearing_header_is_integrity() {
    let h = Home::new();
    drop(h.create());
    // A rotation-style recovery file, the remaining kind of snapshot file.
    {
        let s = h.unlock_with_snapshots();
        let copy = s.begin_destructive(false, None).unwrap();
        s.finish_destructive(copy).unwrap();
    }
    let listed = run(&h, &["snapshot", "ls"]);
    assert_eq!(listed.code, 0, "{}", listed.stderr);
    let id = &listed.stdout.lines().next().unwrap()[..32];
    let archive_path = h.snapshots().join(format!("{id}.qvsnap"));
    let original = std::fs::read(&archive_path).unwrap();
    let len = u32::from_be_bytes(original[8..12].try_into().unwrap()) as usize;
    let mut header = original[12..12 + len].to_vec();
    assert_eq!(header[0], 0xab);
    let mac_field = header.len() - 35;
    assert_eq!(&header[mac_field..mac_field + 3], &[11, 0x58, 32]);
    header[0] = 0xac;
    header.splice(mac_field..mac_field, [10, 0xa0]);
    let mut crafted = original[..8].to_vec();
    crafted.extend_from_slice(&(header.len() as u32).to_be_bytes());
    crafted.extend_from_slice(&header);
    crafted.extend_from_slice(&original[12 + len..]);
    std::fs::write(&archive_path, crafted).unwrap();
    let target = h.vaults().join("review-restored.qvault");
    let before = std::fs::read(h.vault()).unwrap();
    let anchors: Vec<_> = std::fs::read_dir(h.anchors())
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    let result = run(&h, &["snapshot", "restore", id, &arg(&target)]);
    // Spec 0011 requires integrity classification for slot-bearing selection.
    assert_eq!(result.code, 7, "{}", result.stderr);
    assert!(!target.exists());
    assert_eq!(std::fs::read(h.vault()).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(h.anchors()).unwrap().count(),
        anchors.len()
    );
    for (path, bytes) in anchors {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}
