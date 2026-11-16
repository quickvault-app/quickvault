// SPDX-License-Identifier: AGPL-3.0-or-later

//! Helpers shared by the core integration tests.

#![allow(dead_code)]

use qv_core::{GroupHandle, Session};

// This deliberately collects the small lifecycle fixture for comparison.
// Product callers consume a page at a time.
pub fn collect<T>(
    mut next: impl FnMut(
        Option<qv_core::ViewCursor>,
    ) -> Result<qv_core::ViewPage<T>, qv_core::CoreError>,
) -> Vec<T> {
    let mut cursor = None;
    let mut out = Vec::new();
    loop {
        let page = next(cursor).unwrap();
        out.extend(page.items);
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    out
}

/// Everything the views show, revealed values included, as text.
pub fn describe(s: &Session) -> Vec<String> {
    fn walk(s: &Session, g: GroupHandle, depth: usize, out: &mut Vec<String>) {
        let mut l = qv_core::Listing {
            groups: Vec::new(),
            entries: Vec::new(),
        };
        for item in collect(|cursor| {
            s.list_page(
                g,
                cursor,
                qv_core::PageLimit::default(),
                &qv_core::CancelToken::new(),
            )
        }) {
            match item {
                qv_core::SearchHit::Entry(e) => l.entries.push(e),
                qv_core::SearchHit::Group(g) => l.groups.push(g),
            }
        }
        for e in &l.entries {
            let d = s.entry(e.handle).unwrap();
            let mut line = format!(
                "{}entry {} [{}]",
                "  ".repeat(depth),
                d.title.expose(),
                d.type_hint.expose()
            );
            for f in &d.fields {
                let v = match &f.value {
                    Some(v) => String::from_utf8(v.expose_secret().to_vec()).unwrap(),
                    None => format!(
                        "<{}>",
                        String::from_utf8(s.reveal(f.handle).unwrap().expose_secret().to_vec())
                            .unwrap()
                    ),
                };
                line.push_str(&format!(" {}={}", f.name.expose(), v));
            }
            if let Some(n) = &d.notes {
                line.push_str(&format!(" notes={}", n.expose()));
            }
            out.push(line);
        }
        for c in &l.groups {
            out.push(format!("{}group {}", "  ".repeat(depth), c.name.expose()));
            walk(s, c.handle, depth + 1, out);
        }
    }
    let o = s.overview().unwrap();
    let mut out = vec![format!(
        "{} entries={} groups={} trash={} conflicts={}",
        o.name.expose(),
        o.entries,
        o.groups,
        o.in_trash,
        o.conflicts
    )];
    walk(s, s.root().unwrap(), 0, &mut out);
    let mut trash: Vec<String> = collect(|cursor| {
        s.trash_page(
            cursor,
            qv_core::PageLimit::default(),
            &qv_core::CancelToken::new(),
        )
    })
    .iter()
    .map(|t| format!("trash {}", t.name.expose()))
    .collect();
    trash.sort();
    out.extend(trash);
    out.extend(
        collect(|cursor| {
            s.recovered_page(
                cursor,
                qv_core::PageLimit::default(),
                &qv_core::CancelToken::new(),
            )
        })
        .iter()
        .map(|e| format!("recovered {}", e.title.expose())),
    );
    out
}
