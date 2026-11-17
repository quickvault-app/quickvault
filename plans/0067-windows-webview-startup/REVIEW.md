# Review notes — Plan 0067: Windows WebView2 startup origins

| Verdict | Checked | Diff checked |
|---------|---------|--------------|
| Fresh-context approval pending; same-session source and verification notes below | 2026-11-17 | Uncommitted plan 0067 changes |

## Summary

The repair reuses the existing bootstrap URL for navigation/IPC and its origin
for media requests. Windows gains its exact mapped HTTP URL; Linux and Android
retain their existing URLs. The native resource handler remains strict about
Wry's restored custom-scheme base. This is a same-session check, not an
independent audit or a fresh-context approval; leave the plan at Implemented
once verification finishes.

## Plan conformance

| Step | Done | Notes |
|------|------|-------|
| 1 | yes | Spec clarification preceded code; upstream archive hash verified. |
| 2 | yes | Shared platform URL, exact-origin regression tests and Windows CI step. |
| 3 | yes | Documentation and verification recorded; native evidence explicitly pending. |
| 4 | yes | Wine hang reproduces in a plain Wry host before navigation; no further product patch supported by this result. |

Out-of-scope code changes: None. `MESSAGE.txt` is the owner's separate verbatim
note request; `dump.png` is the owner's existing untracked report. Neither is
part of the framework repair.

## Findings

No additional code defect found in the scoped source check. Exact string
matching preserves rejection of other schemes, lookalike hosts, credentials,
ports, extra paths, queries and fragments. Invalid inputs do not consume the
navigation/initialization flags or IPC queue permits. Repeat initialization
remains rejected. No CSP, bridge methods, input bounds, native resource
admission, preview capability, Host/range validation or secret lifetime changed.

Wry 0.53.5's WebView2 adapter forwards mapped document URLs to navigation and
IPC, and reverses the mapping for native custom-protocol requests. Reusing the
bootstrap constant therefore fixes the Windows mismatch without adding an HTTP
wildcard or accepting a remote page.

The owner's continued failure was confirmed to be Wine on Linux. A separate
plain Wry/WebView2 host, with no QuickVault navigation/IPC policy or vault use,
also fails to complete WebViewBuilder::build or reach navigation/page scripts.
This reproduces on private Xvfb and the normal display, including a diagnostic
`--disable-gpu` trial. That locates this Wine failure before the repaired URL
checks, without proving its precise internal cause or native Windows behavior.
Debugger attachment was refused; upstream reports are leads only. No additional
product source changed after the previously passing gates.

## Fixes applied by reviewer

None.

## Gates

Full logs: `/tmp/qv-windows-origin.KjiYnm/`.

| Check | Result |
|-------|--------|
| `sh scripts/check-spdx.sh` | pass |
| `cargo fmt --check` | pass |
| `cargo clippy --all-targets --locked -- -D warnings` | pass |
| `cargo test --locked` | pass: 1,091 passed, 8 ignored; KDF concurrency default one |
| `sh scripts/check-matrix.sh` | pass |
| `python3 scripts/check-supply-chain.py` | pass |
| actionlint 1.7.7, `.github/workflows/ci.yml` | pass |
| Focused maintained-framework `--lib hardened`, Linux | pass: 15 tests |
| Same focused tests, Windows GNU under Wine | pass: 15 tests; regular owner prefix with locked dependency's WebView2Loader on DLL path |
| Windows GNU workspace and desktop-app Clippy | pass |
| Linux validation-feature app Clippy | pass |
| Linux real-engine tests | pass: all three, including real Linux services |
| Windows synthetic preview build | pass; validation-only unused-mut warnings in unchanged fixture code |
| Windows synthetic preview visibility under Wine | inconclusive: Wine nested exceptions and no visible window recorded; launch exits 143, with read-only registry warnings on shutdown |
| Native Windows MSVC CI / attended startup and playback | pending; not available locally |
| `cargo deny check`, `cargo vet --locked` | not required: no dependency or lockfile changes |

Optional isolated-vendor `cargo fmt --check` reports existing formatting in
app.rs, lib.rs and an unchanged hardened.rs CSP assertion. No new hunk was
flagged; preserve upstream/previous patch formatting outside this repair.
The maintained dependency also retains its existing `show_toast` dead-code
warning. Neither affects the passing required workspace formatting/lint gates.

## Documentation

- [x] CHANGELOG.md
- [x] ARCHITECTURE.md unchanged: no component, framework version or trust-boundary split
- [x] Spec clarification, Changelog and index; whole-spec statuses unchanged
- [x] Exact three-file vendor patch and provenance in third-party/README.md
- [x] Plan ticks, execution log, plans/README.md row

## Remaining review and platform evidence

Fresh-context review must issue the formal verdict. Native Windows startup,
first-run UI and media playback need an attended retest using a new build.
Wine and component tests are development evidence and close no native Windows
release gate. No unresolved design decision or security claim is introduced.
