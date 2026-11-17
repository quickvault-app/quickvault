# Plan 0066 — Publish separate Alpine 3.22 and Edge packages

| Status | Created | Approved | Specs |
|--------|---------|----------|-------|
| Implemented | 2026-11-16 | 2026-11-16 — owner's explicit request | 0012 §3 |

## Intent

Add an Alpine Edge APK to tagged releases because the owner uses Edge.
The existing Alpine 3.22 package links libxdo.so.3, while current Edge
provides libxdo.so.4. Give each build a clear target name and check that
the package installs on that target before upload.

## Scope

**In scope:** Alpine packaging, release matrix and installation validation,
artifact names, release instructions and correction of the 1.0.0 record's
overbroad "3.22+" compatibility claim.

**Out of scope:** Rust/application behavior, vault or protocol formats,
dependency/lockfile changes, version bumps, commits, tags, remote release
changes and adding an artifact to the already published 1.0.0 release.

## Spec basis

| Behaviour | Spec | Spec status | Notes |
|-----------|------|-------------|-------|
| Locked release builds, accurate platform records and SHA-256 checksums | 0012 §3 | Accepted | Keep existing release signing policy and publication gates. |
| Additional Edge distribution build | Owner request, 2026-11-16 | Approved | Same application and features, compiled against Edge libraries. |

**Spec changes required:** None; this adds a distribution artifact without
changing application behavior or any format.

## Decisions needed

None. The owner explicitly requested the Edge package. The filename and
matrix layout are implementation details within that request.

## Design

**Components touched:** Repository packaging and GitHub release automation.
No application component or trust boundary changes.

**Interfaces:** `package-alpine.sh [3.22|edge]`, defaulting to `3.22` for
existing container invocations. Reject unsupported targets and extra
arguments before installing tools or compiling. Emit
`quickvault-<version>-r0-alpine-<branch>-<arch>.apk`.

**Data and control flow:** A two-entry release matrix runs the same script
inside `alpine:3.22` or `alpine:edge`, passing the matching branch argument.
Each runner has its own workspace and outputs. A second clean container
of the same image installs the produced APK with normal dependency
resolution, runs `qv --version` and checks the desktop binary with `ldd`.
Upload only after these checks pass. `publish` continues to depend on the
whole Alpine job, so both matrix entries must succeed and are included in
the existing SHA256SUMS step. Keep `fail-fast: false` to retain evidence
from both targets when one fails.

**Secrets and hostile input:** Keep existing disposable Alpine signing keys
and container-only public-key registration. No Android signing-key or
vault access. Target arguments are limited to the two matrix values;
quote their use in Docker commands. Runtime dependency verification never
uses a symlink alias, fake provider or dependency override.

**Errors:** Invalid targets fail with a usage error. Compilation,
packaging, installation, CLI execution or loader failures prevent upload.

**Dependencies:** No product or build-tool dependency additions; reuse
Docker, Alpine apk/abuild, Cargo and the existing pinned checkout action.
Edge deliberately follows its repository's current native libraries.

## Steps

### Step 1 — Build and validate both Alpine targets

- [x] **Do:** Add the validated branch argument and branch-qualified
  filename. Add the release matrix and clean-container install/loader
  checks before upload; keep the existing publish dependency.
- **Files:** `crates/qv-app/scripts/package-alpine.sh`,
  `.github/workflows/release.yml`.
- **Done when:** Matrix entries target distinct files, invalid arguments
  have no build side effects, and failed installation prevents upload.
- **Verify:** Shell syntax and argument rejection; parse/lint the workflow;
  exercise packaging names and checks with local fixtures, actual Alpine
  tooling, and the previously built Edge APK. Run both full Docker builds
  if the daemon is available; otherwise explicitly record the CI-only gap.

### Step 2 — Document targets and verify the final tree

- [x] **Do:** Document the two packages and matching repositories, correct
  the existing release's compatibility claim, update the architecture's
  packaging technology entry and README navigation, and record evidence.
- **Files:** `docs/RELEASE.md`, `docs/release-notes/1.0.0.md`, `README.md`,
  `ARCHITECTURE.md`, `CHANGELOG.md`, this plan and `plans/README.md`.
- **Done when:** Instructions distinguish stable and Edge packages without
  claiming that the published 1.0.0 assets changed. All required gates pass.
- **Verify:** `sh scripts/check-spdx.sh`, `cargo fmt --check`,
  `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`,
  `sh scripts/check-matrix.sh`, `git diff --check`.

## Test plan

- Actual dependency installation on matching clean Alpine images in CI.
- Reject unsupported/extra script arguments before apk/Cargo calls.
- Verify matrix images and arguments, distinct asset names and ordering of
  build → install/loader check → upload → aggregate publish.
- Retain the existing local Edge APK dependency/loader/install-simulation
  evidence and the Alpine 3.22/Edge dependency audit as supporting evidence.
- All AGENTS.md gates on the final tree. No new 0012 §1 row or format vector:
  application behavior is unchanged. No deny/vet rerun: no dependencies change.

## Risks and stop conditions

- Edge can change ABI; rebuilding on Edge and checking installation detects
  current mismatches but does not promise compatibility with future Edge.
- Docker is installed locally but its daemon is unavailable. Local checks
  cannot be represented as full fresh-container release-build evidence.
- Stop if implementation requires an application dependency or behavior change.
- Fresh-context review remains separate from this implementation session.

## Documentation

- [x] `CHANGELOG.md` — Unreleased entry.
- [x] `ARCHITECTURE.md` §6 — distribution build targets and validation.
- [x] `docs/RELEASE.md` / `README.md` — artifact choice and release navigation.
- [x] `docs/release-notes/1.0.0.md` — correct compatibility, retain historical names.
- [x] Specs and `AGENTS.md` — no change.
- [x] Plan index and execution evidence.

## Revisions

- 2026-11-16: Planned and approved within the owner's explicit instruction
  to add an Alpine Edge package; no additional owner decision is needed.

## Execution log

- 2026-11-16: Clean tree at start. Existing published APK's 17 declared
  shared libraries match its binaries. The complete 3.22 dependency tree
  resolves; Edge's only missing direct library is libxdo.so.3. A local
  release build linking libxdo.so.4 was packaged and passed both loader
  checks and an install simulation on this Edge system. This is supporting
  evidence for the new release target, not a change to published assets.
- 2026-11-16: Both steps implemented. The Alpine job now has independent
  `3.22` and `edge` matrix entries with matching Docker images and script
  arguments; each installs its package and checks the CLI and both loaders
  before upload. Aggregate publication still waits for the whole matrix.
  The packaging script defaults to 3.22, rejects unsupported/extra arguments
  before build actions and includes the branch in the output filename.
- 2026-11-16: Focused checks pass (`/tmp/qv66-focused.log`): shell syntax,
  early argument rejection, actual shell filename expansion for both targets
  and the default, matrix/image alignment, installation before upload and
  publication dependency. The previous script fails both target-name
  expectations. Actionlint 1.7.12 accepts the workflow
  (`/tmp/qv66-actionlint.log`; optional shellcheck/pyflakes integrations are
  unavailable and were disabled; shell syntax was checked separately).
- 2026-11-16: Local Edge package checks pass again on unchanged application
  sources: both native loader checks, all 17 metadata/ELF dependency matches
  and a complete Edge install simulation (`/tmp/qv66-edge-package-check.log`).
  Its CLI prints `qv 1.0.0` with temporary XDG directories
  (`/tmp/qv66-cli-version.log`). No vaults were accessed.
- 2026-11-16: Required local gates pass: SPDX, format, all-target locked
  Clippy with warnings denied, full locked workspace tests, matrix and
  whitespace checks (`/tmp/qv66-{spdx,fmt,clippy,tests,matrix,diff}.log`).
  No manifests, lockfiles or application sources changed. Subsequent edits
  only completed documentation; final SPDX/format/matrix/whitespace and
  workflow checks are repeated before handoff.
- 2026-11-16: Docker's daemon is unavailable (`/tmp/qv-edge-docker.log`), so
  the full fresh-container builds/installs are pending the tagged CI run.
  This session's consistency check found no further code issue; a fresh
  context review remains pending, so this plan is Implemented, not Reviewed.
  No commit, push, tag or published-release change was made.
