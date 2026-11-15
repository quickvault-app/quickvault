// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trash is a separate address scope; permanent deletion always confirms.
use crate::{
    cli::Ctx,
    exit::{AMBIGUOUS, CliError, NOT_FOUND},
    json::Json,
    out::{Out, escape_str},
    term,
};
use clap::ArgMatches;
use qv_core::{CoreError, ItemHandle, Session};

/// A deletion's confirmation, naming the snapshots it also removes (0007
/// §3.3, 0011 §4).
pub(crate) fn snapshot_question(question: &str, holding: usize, keep: bool) -> String {
    match (holding, keep) {
        (0, _) => question.to_owned(),
        (n, true) => format!(
            "{question} {n} snapshot(s) still hold this data and are kept (--keep-snapshots)."
        ),
        (n, false) => format!("{question} {n} snapshot(s) holding this data are deleted too."),
    }
}

fn resolve(s: &Session, addr: &str) -> Result<ItemHandle, CliError> {
    let ids = addr
        .strip_prefix('@')
        .map(|prefix| {
            s.find_id(prefix, true).map_err(|e| match e {
                CoreError::InvalidInput => CliError::usage("an @id needs 6 to 32 hex digits"),
                e => e.into(),
            })
        })
        .transpose()?;
    let mut hits = Vec::new();
    each(s, |t| {
        if ids
            .as_ref()
            .map_or_else(|| t.name.expose() == addr, |ids| ids.contains(&t.item))
        {
            hits.push(t.item);
        }
        Ok(())
    })?;
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(CliError::new(NOT_FOUND, "trashed item not found")),
        many => {
            let mut message = format!(
                "{} matches multiple trashed items; use an ID:",
                escape_str(addr)
            );
            for item in many {
                message.push_str(&format!("\n  @{}", item.id_hex()));
            }
            Err(CliError::new(AMBIGUOUS, message))
        }
    }
}

pub(crate) fn run(ctx: &Ctx, m: &ArgMatches) -> Result<(), CliError> {
    let (cmd, args) = m
        .subcommand()
        .ok_or_else(|| CliError::usage("missing trash command"))?;
    if cmd == "empty" && !ctx.yes {
        if ctx.password_stdin {
            return Err(CliError::usage(
                "trash empty with --password-stdin needs --yes",
            ));
        }
        term::stdio_tty()?;
    }
    let s = ctx.unlock()?;
    match cmd {
        "ls" => {
            let mut out = Out::stdout();
            if ctx.json {
                let mut j = Json::new(&mut out);
                j.begin_obj().key("items").begin_arr();
                each(&s, |t| {
                    let kind = match t.item {
                        ItemHandle::Entry(_) => "entry",
                        ItemHandle::Group(_) => "group",
                    };
                    j.begin_obj()
                        .key("id")
                        .str(&t.item.id_hex())
                        .key("kind")
                        .str(kind)
                        .key("name")
                        .str(t.name.expose())
                        .key("deleted_at")
                        .time(t.deleted_at.map(|t| t.0))
                        .key("conflict")
                        .bool(t.conflict)
                        .end_obj();
                    j.flush()
                })?;
                j.end_arr().end_obj();
            } else {
                each(&s, |t| {
                    out.raw("@");
                    out.raw(&t.item.id_hex());
                    out.raw("  ");
                    out.raw(match t.item {
                        ItemHandle::Entry(_) => "entry",
                        ItemHandle::Group(_) => "group",
                    });
                    out.raw("  ");
                    out.text(t.name.expose());
                    out.raw("  ");
                    if let Some(at) = t.deleted_at {
                        out.raw(&crate::out::rfc3339(at.0));
                    } else {
                        out.raw("unknown");
                    }
                    if t.conflict {
                        out.raw("  [conflict]");
                    }
                    out.raw("\n");
                    out.flush()
                })?;
            }
            if ctx.json {
                out.raw("\n");
            }
            out.flush()?;
        }
        "restore" => {
            let item = resolve(
                &s,
                args.get_one::<String>("item")
                    .map(String::as_str)
                    .unwrap_or_default(),
            )?;
            crate::signals::check()?;
            s.restore(item)?;
        }
        "empty" => {
            // No recovery copy: the trash was the safety net (0007 §3.2). The
            // snapshots holding the deleted items go too unless kept (§3.3).
            let keep = args.get_flag("keep-snapshots");
            if s.expiry_summary()?.clock_anomaly {
                crate::out::eprint(
                    "qv: warning: vault dates are ahead of this clock; check it before deletion\n",
                );
            }
            let holding = s.snapshots_holding(qv_core::DeletionTarget::Trash)?;
            let question = snapshot_question(
                "Permanently delete all non-conflicted trash? Independent copies may remain.",
                holding,
                keep,
            );
            if !ctx.yes && !term::stdio_tty()?.confirm(&question)? {
                return Err(CliError::cancelled());
            }
            crate::signals::check()?;
            let report = s.empty_trash(keep)?;
            if report.skipped_conflicts != 0 {
                crate::out::eprint(&format!(
                    "qv: {} conflicted item(s) kept in trash\n",
                    report.skipped_conflicts
                ));
            }
        }
        _ => return Err(CliError::usage("unknown trash command")),
    }
    s.lock();
    Ok(())
}

fn each(
    s: &Session,
    mut write: impl FnMut(qv_core::TrashItem) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let revision = s.view_revision()?;
    let mut cursor = None;
    loop {
        let page = s.trash_page(
            cursor,
            qv_core::PageLimit::default(),
            &crate::signals::token(),
        )?;
        if page.revision != revision {
            return Err(CoreError::StaleHandle.into());
        }
        for item in page.items {
            write(item)?;
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    if s.view_revision()? != revision {
        return Err(CoreError::StaleHandle.into());
    }
    Ok(())
}
