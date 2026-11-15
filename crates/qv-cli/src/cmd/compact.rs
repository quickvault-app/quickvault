// SPDX-License-Identifier: AGPL-3.0-or-later

//! `qv compact [--json]` (spec 0011 §4, 0004 §1.4): the storage overview,
//! then a rewrite of the vault file without its free pages. It changes no
//! data, so it asks nothing.

use clap::ArgMatches;
use qv_core::StorageInfo;

use crate::cli::Ctx;
use crate::exit::CliError;
use crate::json::Json;
use crate::out::Out;

pub(crate) fn run(ctx: &Ctx, _m: &ArgMatches) -> Result<(), CliError> {
    let session = ctx.unlock()?;
    let info = session.storage()?;
    crate::signals::check()?;
    let cancel = crate::signals::token();
    let done = session.compact(Some(&cancel))?;
    let mut out = Out::stdout();
    if ctx.json {
        let mut j = Json::new(&mut out);
        j.begin_obj()
            .key("file_bytes")
            .uint(info.file_bytes)
            .key("free_bytes")
            .uint(info.free_bytes)
            .key("snapshot_bytes")
            .uint(info.snapshot_bytes)
            .key("disk_free");
        match info.disk_free {
            Some(n) => j.uint(n),
            None => j.null(),
        };
        j.key("compacted_from");
        match done {
            Some((from, _)) => j.uint(from),
            None => j.null(),
        };
        j.key("compacted_to");
        match done {
            Some((_, to)) => j.uint(to),
            None => j.null(),
        };
        j.end_obj();
        out.raw("\n");
    } else {
        human(&mut out, &info);
        match done {
            Some((from, to)) => out.raw(&format!("Compacted: {from} -> {to} bytes\n")),
            None => out.raw("Nothing to compact.\n"),
        };
    }
    out.flush()?;
    session.lock();
    Ok(())
}

/// The overview: sizes only, printed as program text.
fn human(o: &mut Out, s: &StorageInfo) {
    o.raw(&format!("Vault file: {} bytes\n", s.file_bytes));
    o.raw(&format!("Reclaimable: {} bytes\n", s.free_bytes));
    o.raw(&format!("Snapshots: {} bytes\n", s.snapshot_bytes));
    match s.disk_free {
        Some(n) => o.raw(&format!("Free disk space: {n} bytes\n")),
        None => o.raw("Free disk space: unknown\n"),
    };
}
