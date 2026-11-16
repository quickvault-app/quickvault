// SPDX-License-Identifier: AGPL-3.0-or-later

//! Makes the 1.0 golden vault and archive (spec 0015 §6, plan 0064).
//!
//! ```text
//! golden_vault <new dir>
//! ```
//!
//! Writes `<dir>/1.0.qvault`, its anchor directory `<dir>/anchors/` and a
//! self-contained archive `<dir>/1.0.qvsnap`, through core's public API, with
//! the published password [`PASSWORD`]. It was run once for 1.0 and the
//! output is committed under `crates/qv-store/tests/golden/`; later builds
//! must open those files, never regenerate them. The expected contents are
//! checked by `crates/qv-core/tests/golden.rs`.

use std::path::Path;
use std::process::ExitCode;

use qv_core::{
    AnchorDir, Core, CoreConfig, FieldKind, FieldValue, ItemHandle, MasterPassword, SnapshotDir,
    SnapshotPolicy, Text,
};

/// Published with the golden files; not a secret.
const PASSWORD: &[u8] = b"quickvault 1.0 golden vault, published test password";

type R<T> = Result<T, String>;

fn err<E: core::fmt::Debug>(what: &'static str) -> impl FnOnce(E) -> String {
    move |e| format!("{what}: {e:?}")
}

fn text(s: &str) -> R<Text> {
    Text::new(s).map_err(err("text"))
}

/// The attachment content: 70,000 bytes (two chunks), fixed.
fn attachment() -> Vec<u8> {
    (0..70_000u32).map(|i| (i % 251) as u8).collect()
}

fn run(dir: &Path) -> R<()> {
    if dir.exists() {
        return Err(format!("{} exists", dir.display()));
    }
    qv_core::create_private_dirs(dir).map_err(err("create directory"))?;
    let vault = dir.join("1.0.qvault");
    let snapshots = SnapshotPolicy::new(
        SnapshotDir::open(&dir.join("snapshots")).map_err(err("snapshot directory"))?,
    );
    let core = Core::new(
        CoreConfig::new(AnchorDir::open(&dir.join("anchors")).map_err(err("anchors"))?)
            .with_snapshots(snapshots),
    );
    let password = MasterPassword::from_utf8(PASSWORD).map_err(err("password"))?;
    let s = core
        .create(&vault, &password, "Golden")
        .map_err(err("create"))?
        .session;
    let root = s.root().map_err(err("root"))?;
    let group = |parent, name| -> R<_> {
        s.save_group(s.new_group(parent, name).map_err(err("new group"))?)
            .map_err(err("save group"))
    };
    let work = group(root, "Work")?;
    let home = group(root, "Home")?;
    let old = group(work, "Old")?;

    let entry = |g,
                 kind: &str,
                 title: &str,
                 fields: &[(&str, FieldKind, &str)],
                 notes: Option<&str>|
     -> R<_> {
        let mut e = s.new_entry(g, kind, title).map_err(err("new entry"))?;
        for (name, kind, value) in fields {
            e.add_field(name, *kind, FieldValue::text(value).map_err(err("value"))?)
                .map_err(err("field"))?;
        }
        if let Some(n) = notes {
            e.entry_mut().notes = Some(text(n)?);
        }
        s.save_entry(e).map_err(err("save entry"))
    };
    let mail = entry(
        work,
        "login",
        "Mail",
        &[
            ("Username", FieldKind::Text, "me@example.invalid"),
            ("Password", FieldKind::Secret, "s3cret"),
            ("Website", FieldKind::Url, "https://mail.example.invalid"),
            ("Email", FieldKind::Email, "me@example.invalid"),
            (
                "One-time code",
                FieldKind::Totp,
                "otpauth://totp/Mail?secret=JBSWY3DPEHPK3PXP",
            ),
        ],
        Some("recovery codes in the safe"),
    )?;
    entry(
        home,
        "card",
        "Card",
        &[
            ("Cardholder", FieldKind::Text, "A. Holder"),
            ("Number", FieldKind::Secret, "4111111111111111"),
            ("Expires", FieldKind::Date, "2030-01"),
            ("CVV", FieldKind::Secret, "123"),
        ],
        None,
    )?;
    let wifi = entry(
        home,
        "wifi",
        "Wifi",
        &[("Password", FieldKind::Secret, "hunter22")],
        None,
    )?;
    entry(
        home,
        "note",
        "Note",
        &[("Text", FieldKind::Multiline, "line 1\nline 2")],
        None,
    )?;
    entry(
        old,
        "login",
        "Legacy",
        &[("Password", FieldKind::Secret, "1234")],
        None,
    )?;
    let tmp = entry(home, "login", "Temporary", &[], None)?;

    s.add_attachment(
        mail,
        "readme.txt",
        "text/plain",
        &mut attachment().as_slice(),
        None,
    )
    .map_err(err("attachment"))?;

    // History, move, trash, restore, purge.
    for title in ["Mail (old)", "Mail (work)"] {
        let mut e = s.edit_entry(mail).map_err(err("edit"))?;
        e.entry_mut().title = text(title)?;
        s.save_entry(e).map_err(err("save edit"))?;
    }
    s.move_item(ItemHandle::Entry(wifi), work)
        .map_err(err("move"))?;
    s.move_item(ItemHandle::Group(old), home)
        .map_err(err("move group"))?;
    s.delete(ItemHandle::Group(old), true)
        .map_err(err("trash group"))?;
    s.restore(ItemHandle::Group(old)).map_err(err("restore"))?;
    s.delete(ItemHandle::Entry(tmp), false)
        .map_err(err("trash entry"))?;
    let copy = s
        .begin_destructive(false, None)
        .map_err(err("begin purge"))?;
    s.purge(&[ItemHandle::Entry(tmp)], false)
        .map_err(err("purge"))?;
    s.finish_destructive(copy).map_err(err("finish purge"))?;
    s.delete(ItemHandle::Group(old), true)
        .map_err(err("trash group"))?;
    let mut st = s.edit_settings().map_err(err("settings"))?;
    st.meta_mut().name = text("Golden vault")?;
    s.save_settings(st).map_err(err("save settings"))?;

    let export = dir.join("export");
    let report = s
        .export_archive(&export, true, None)
        .map_err(err("export"))?;
    if let Some(e) = report.completion_error {
        return Err(format!("export completion: {e:?}"));
    }
    s.lock();
    std::fs::rename(
        export.join(format!("{}.qvsnap", report.archive_id)),
        dir.join("1.0.qvsnap"),
    )
    .map_err(err("move archive"))?;
    std::fs::remove_dir(&export).map_err(err("remove export directory"))?;
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, dir] = args.as_slice() else {
        eprintln!("usage: golden_vault <new dir>");
        return ExitCode::from(2);
    };
    match run(Path::new(dir)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("golden_vault: {e}");
            ExitCode::FAILURE
        }
    }
}
