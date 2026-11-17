# Plan 0067 — Windows WebView2 startup origins

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented | 2026-11-17 | 2026-11-17 — owner's explicit request to fix the diagnosed Windows origin bug | 0008 §4.2–4.3, 0009 §5.1, 0012 §1/§3 |

## Intent

Fix the Windows desktop app starting WebView2 without showing its window.
The bundled document's mapped HTTP URL must pass navigation and initialization
IPC checks; its origin must also pass preview request checks.

## Scope

**In scope:** exact platform document/origin checks, regression tests, native
Windows CI coverage of the maintained framework tests and repair documentation.

**Out of scope:** changing the framework/engine, visibility timing, CLI behavior,
formats, dependencies, release publication or the SIGINT cleanup diagnostic.

## Spec basis

| Behaviour | Spec | Spec status | Notes |
|-----------|------|-------------|-------|
| Bundled document only, restrictive CSP and bounded bridge | 0008 §4.2–4.3 | Draft; owner-approved webview boundary in 0030, specific repair approved now | Preserve exact matching and one initial navigation. |
| Windows WebView2 desktop | 0009 §5.1 | Draft; owner-approved Windows profile in 0052 | Correct the existing profile's URL handling. |
| Targeted hostile-input tests and honest platform evidence | 0012 §1/§3 | Accepted | Tests do not prove security or native window startup. |

**Spec changes required:** clarify platform document URLs and preview origins
in 0008 §4 before code; add its Changelog and specs index note. This repair
implements the existing bundled-only contract, without changing spec status.

## Decisions needed

None. The owner explicitly requested the previously diagnosed repair. Proceed
under that authorization without a second approval question.

## Design

**Components touched:** maintained dioxus-desktop protocol/bootstrap, navigation,
IPC and loopback-media origin validation; Windows CI. No component split.

**Interfaces:** keep public signatures. Make protocol's existing `BASE_URI`
crate-visible and reuse it in `hardened::local_document` and media origin checks.
Its values remain Linux `dioxus://index.html/`, Windows
`http://dioxus.index.html/`, Android `https://dioxus.index.html/`.
Preview origins omit the trailing slash. Preserve existing opaque `null`
acceptance only on non-Android/non-Windows platforms, without any CORS grant.
Wry reverses custom-protocol resource URLs before the native resource handler;
keep that handler's exact `dioxus://index.html/` check.

**Data and control flow:** initial mapped navigation → bundled bootstrap →
origin-checked initialize IPC → existing window visibility transition. No
storage or transaction changes. Media requests retain numeric Host, capability,
range, bounds and revocation checks. Reject lookalike hosts, alternate schemes,
ports, credentials, extra paths, query/fragment and repeat navigation.

**Secrets and hostile input:** preserve all IPC bounds, queue accounting, CSP,
no-store and redaction. Add no logging or secret allocation. Keep the native
resource URL distinct from the web engine's document URL.

**Errors:** preserve refusal behavior; no new error category or exposed input.

**Dependencies:** None. Verify the retained upstream archive hash and inventory
the existing vendor diff before editing; record the exact new patch paths.

## Steps

### Step 1 — Specify the platform mapping

- [x] **Do:** Clarify exact document/origin behavior in 0008 §4, its Changelog
  and specs index. Verify upstream archive provenance and patch inventory.
- **Files:** `specs/0008-memory-and-side-channels.md`, `specs/README.md`.
- **Done when:** accepted URLs and unchanged rejection rules are explicit.
- **Verify:** inspect spec diff and SHA-256-verified archive comparison.

### Step 2 — Repair and test origin admission

- [x] **Do:** Reuse bootstrap's platform URL for navigation/IPC and media
  origin admission. Make existing tests platform-aware and extend rejection,
  initialization state and CORS coverage. Run hardened tests on Windows CI.
- **Files:** `third-party/dioxus-desktop/src/{protocol,hardened,hardened_media}.rs`,
  `.github/workflows/ci.yml`.
- **Done when:** target's exact URL initializes once; other origins never
  consume initialization state or receive media/CORS access.
- **Verify:** `cargo test --manifest-path third-party/Cargo.toml --locked -p dioxus-desktop --no-default-features --features tokio_runtime --lib hardened`;
  Windows cross-target Clippy/tests where available; Linux real-engine tests.

### Step 3 — Document and verify

- [x] **Do:** Record repair, provenance, current checks and native Windows
  retest needs; review the final patch against the spec.
- **Files:** `CHANGELOG.md`, `third-party/README.md`, this plan, `REVIEW.md`,
  `plans/README.md`, `plans/0052-windows-platform/WINDOWS-GATES.md`.
- **Done when:** gates pass; absent native evidence is explicit.
- **Verify:** final gates below and diff review.

### Step 4 — Investigate the owner's Wine startup failure

- [x] **Do:** Reproduce with the installed WebView2 runtime using a writable
  temporary copy of the owner's prefix. Compare a synthetic plain Wry page
  against QuickVault's synthetic hardened preview, recording fixed startup
  stages and page-loaded success. Keep probes outside the product tree and
  use no vaults. Distinguish a runtime/compatibility failure from an app bug
  before prescribing another product change or workaround.
- **Files:** temporary diagnostic probes/logs under
  `/tmp/qv-windows-origin.KjiYnm/`; plan, review notes and plans index.
- **Done when:** the failing startup stage and the control result are known;
  any concrete product fix receives a plan revision/spec coverage before code.
- **Verify:** synthetic Wine probes and bounded real-window checks. Do not
  weaken the production CSP, origin checks or sandbox to obtain a pass.

## Test plan

- Final gates: `sh scripts/check-spdx.sh`, `cargo fmt --check`,
  `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`.
- Also matrix and supply-chain scripts, focused vendor tests, desktop-feature
  Clippy and Linux validation engine tests (`RUST_TEST_THREADS=1`).
- Windows: `cargo clippy --target x86_64-pc-windows-gnu --all-targets --locked -- -D warnings`,
  plus desktop-feature Clippy and focused vendor tests under Wine if available
  (`umask 077`, `QV_WINE=1`). KDF concurrency stays at its default of one.
- 0012 §1: Webview exposure. Test exact-origin admission and rejected variants;
  preserve existing malformed IPC, replay, queue and media transport tests.
- No file/wire format, crypto vectors or crash ordering change.

## Risks and stop conditions

- Stop if Wry uses an additional undocumented privileged origin, or fixing this
  requires broad origin acceptance, a CSP relaxation or dependency changes.
- Native Windows startup, UI and media playback still require an attended retest;
  cross-compilation, Wine and Linux tests cannot close those release gates.
- Preserve the owner's untracked `dump.png`; never commit, push or branch.

## Documentation

- [x] `CHANGELOG.md` — Windows startup repair.
- [x] `third-party/README.md` — exact patch and provenance.
- [x] Specs — 0008 clarification, Changelog and index; statuses unchanged.
- [x] Windows checklist — reported failure, repair and pending retest.
- `ARCHITECTURE.md` — no component/technology change.
- `AGENTS.md` — no workflow change.

## Revisions

- 2026-11-17: Owner reports the supplied EXE still shows nothing, confirms
  Wine on Linux and explicitly requests continuation. Resume the plan with
  step 4 to identify that failure. Existing staged changes remain intact.
  Product origin repair stays in place pending evidence of another defect.

## Execution log

- 2026-11-17: Architect plan recorded before implementation. Owner's “So,
  can you fix it?” authorizes the diagnosed, bounded repair. Only `dump.png`
  was untracked; no tracked changes were present. No unresolved owner decision.
- 2026-11-17: Step 1 complete. Upstream archive SHA-256 verified as
  `236deb81f1a04de704fb80cf7838c65607441c76914de2e036709540c540a39d`.
  Existing inventory: modified Cargo.toml, build.rs and src/{app,config,edits,
  ipc,lib,protocol,webview}.rs; added UPSTREAM.sha256 and
  src/{hardened,hardened_media}.rs; omitted .cargo_vcs_info.json. License and
  lockfile contents were excluded from this source comparison. Switching to
  the developer role for steps 2–3; no fresh-context review claim.
- 2026-11-17: Step 2 complete. `local_document` and preview Origin checks now
  reuse the existing platform bootstrap URL. Tests retain exact matching,
  one-use initialization/navigation, queue accounting and rejected-origin/CORS
  coverage. Windows CI now runs the isolated maintained-framework tests.
- 2026-11-17: Required gates pass on the repaired source: SPDX, workspace fmt,
  workspace Clippy and `cargo test --locked` (1,091 passed, 8 ignored; 338.7 s;
  KDF concurrency left at one). Matrix and supply-chain scripts pass; actionlint
  1.7.7 passes the CI workflow. Logs: `/tmp/qv-windows-origin.KjiYnm/`.
- 2026-11-17: Focused vendor hardened tests pass on Linux (15) and Windows GNU
  under Wine (15). Windows workspace and desktop-feature Clippy pass, as does
  Linux validation-feature Clippy. All three Linux engine tests pass, covering
  real boundary rejection, shared create/lock/unlock/recovery and Linux services.
- 2026-11-17: Environment setup: added the Windows GNU Rust target and extracted
  Alpine's mingw-w64 toolchain into the temporary log directory, with no repo
  dependency or lockfile change. An initial preview build omitted that temporary
  toolchain from PATH and failed to find dlltool; the corrected build passes.
  The first isolated-prefix Wine attempt could not load WebView2Loader.dll.
  The owner then explicitly authorized the regular WebView2-equipped prefix;
  supplying the locked webview2-com-sys x64 loader on WINEPATH lets all 15 tests
  pass there. The synthetic Windows preview build passes; visibility check next.
- 2026-11-17: Same-session reviewer role checked the small source diff and Wry
  mapping, without claiming fresh-context approval. REVIEW.md records scope,
  tests, existing optional vendor-format/dead-code warnings and pending native
  Windows evidence. Engine versions and WebKitGTK workarounds are unchanged.
- 2026-11-17: Step 3 complete. The Windows synthetic preview builds, but its
  private-Xvfb launch in the owner's regular Wine prefix records WebView2/Wine
  nested exceptions and DRI3 warnings, then exits 143 without a recorded visible
  window. Wine also reports the prefix registry as read-only on shutdown. This
  GUI experiment is inconclusive, not passing startup evidence; no production
  workaround or relaxed browser flags were added. Native Windows startup and
  media retest remain pending. No Wine server or unrelated app was deliberately
  stopped; the harness only terminates its own preview process and X server.
  Source repair, focused Windows tests and required gates are complete. Status
  Implemented; fresh-context review and attended platform evidence remain open.
- 2026-11-17: Owner requested a runnable EXE. Built the regular desktop app
  (not the synthetic preview) with `cargo build --locked --target
  x86_64-pc-windows-gnu -p qv-app --features desktop --bin quickvault` using
  the temporary mingw toolchain (pass, 39.5 s). Packaged the debug EXE,
  the locked webview2-com-sys x64 WebView2Loader.dll, licence, instructions and
  SHA256SUMS in `target/dist/quickvault-windows-test.zip`. PE imports require
  the bundled loader plus Windows system DLLs; no extra compiler runtime DLL
  appeared. ZIP integrity checked. This is an unsigned local development
  artifact; native Windows retest and the MSVC release build remain separate.
- 2026-11-17: Step 4 complete after the owner confirmed the continued failure
  is under Wine on Linux. Copied the 1.4 GiB WebView2-equipped prefix into the
  temporary directory for writable, isolated diagnostics. A temporary plain
  Wry 0.53.5 host uses a visible native window, inline synthetic HTML and a
  fixed IPC success marker; it contains no QuickVault navigation/bridge/CSP
  logic or vault access. It logs native-window creation and entry into
  `WebViewBuilder::build`, but neither completion, navigation, page load nor
  the page-script success marker appears within 35 seconds. The hardened
  synthetic preview likewise shows no window within 35 seconds on private Xvfb.
- 2026-11-17: Repeated the plain control on the normal `DISPLAY=:0` with fresh
  temporary WebView2 data folders, default rendering and then `--disable-gpu`.
  Both stall at the same creation stage for 25 seconds, with Wine nested
  exceptions. This local failure is therefore reproducible without QuickVault's
  origin checks. GDB attachment was refused by the environment (`ptrace:
  Operation not permitted`), so the internal COM wait/callback is not known.
  Upstream WebView2Feedback issues [5720](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5720)
  (Wine rendering) and [5706](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5706)
  (creation completion hangs) were consulted as leads, not identified as the
  proven cause. No sandbox/CSP relaxation, runtime downgrade, dependency patch,
  new production diagnostic or further app-code change was made. Only the
  copied prefix's servers were stopped between trials; owner prefix untouched.
  Logs/probes are under the temporary directory (`wine-probes.log`,
  `probe-normal-results.log`, `plain-wine.log`, `normal-*.log`).
- 2026-11-17: The Wine investigation changes documentation only; product Rust,
  tests, manifests, lockfiles and packaged app remain identical to the passing
  gate inputs above. Origin-repair implementation is complete, but GUI startup
  is not validated on native Windows. Keep that attended retest and fresh
  review pending; do not describe the continued Wine hang as repaired.
