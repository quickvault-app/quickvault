# LAN/2 source and composition review

| Reviewer | Date | Scope / verdict |
|----------|------|-----------------|
| Codex, separate continuation session from the original LAN implementation; also author of the repairs below | 2026-11-16 | Source review completed; two implementation issues repaired; final verification recorded in PLAN.md. No independent-party audit or platform release clearance claimed. |

## Baseline and method

Reviewed `e177ba15801eac0eb7716a247415bc05a72bd605` plus the inherited staged
1.0 repairs and this plan's working changes. `SOURCE-BASELINE.sha256` pins the
reviewed LAN source and tests after repairs. The old 0048 baseline identifies
an earlier tree; it is not evidence for this one.

Read normative 0013 §15 with the surviving exchange §7, 0014 §10, 0012 §3,
0047 LAN-CONTRACT and 0048 REVIEW-PACKAGE. Traced the production drivers into
core authorization, authenticated staging, merge and durable publication;
checked canonical messages and key schedules against the registered arrays;
read the adversarial/component tests and ran the current automated gates.
No external party was contacted. No personal vault or credential was used.

This is a source-level review, not a cryptographic proof, new dependency audit,
memory-forensics assessment or exhaustive state-space exploration. The reviewer
implemented the corrections and therefore cannot claim independent retesting
of those corrections. The owner's 0056 request for another agent session is
addressed; 0012's independent-composition obligation is not silently waived.

## Findings and disposition

1. **Medium — existing-copy initiator did not bound local processing by its
   network deadline. Fixed.** `qv-core/src/lan.rs`, `Session::sync_with_code`,
   constructed `SessionExchange` with `expires=None`. The sync driver capped
   socket I/O, but a sufficiently late/large authenticated transfer could enter
   local verification and commit after that cap. No credential bypass was
   demonstrated; this violated the absolute processing bound in 0013 §15.6.
   Core now creates the deadline before beginning the operation and passes it
   both to `SessionExchange` and `qv_sync::lan::sync_with_code_until`. The old
   sync wrapper still creates its own bounded deadline for other callers.
   `lan_initiator_deadline_covers_connect_and_local_acceptance` checks an
   expired initiation never connects and an expired, fully staged merge cannot
   advance the local sequence. Existing commit/anchor/lost-receipt tests still
   cover preservation of changes that were already durable.

2. **Medium — progress accounting performed quadratic reference scans. Fixed.**
   `qv-store/src/lan_staging.rs`, `LanStaging::peer_bytes`, scanned the complete
   reference plan after each received object. At the supported 381,072-reference
   bound this could require over 145 billion reference visits for a valid
   transfer, wasting its deadline and allowing an authenticated peer to consume
   disproportionate CPU. The plan computes total peer bytes once; successful
   peer appends advance a bounded counter. Local copies and rejected objects
   do not advance it. `lan_staging_progress_counts_only_successful_peer_objects`
   checks mixed local/peer accounting and unchanged progress on corruption.
   No ciphertext, reference, authentication check or wire encoding changed.

No additional critical/high flaw or demonstrated credential-free key release
was found within this scope. Absence of a finding is not proof of security.

## Composition trace

| Review question | Source trace and assessment |
|-----------------|-----------------------------|
| Fixed engine and local registration | `qv-crypto/src/lan_auth_engine.rs` fixes Ristretto255/TripleDh/SHA-512 and Argon2id v1.3, 262144 KiB/t3/p4, 64-byte output, zero salt16. `lan_auth.rs` supplies explicit identities/context and Stretch on registration and client finish. Core verifies the working slot against VK and a unique encrypted head first. No verifier/setup is imported or persisted. |
| Fallible randomness | `CheckedRng` latches source failure and uses termination poison only inside the component. `healthy` and `engine_operation` reject the entire output after every relevant random component operation. Existing every-draw failure tests cover wrapper and selected scalar paths. X-Wing draws fallibly through protected seeds. |
| Cleanup | Inter-flight OPAQUE serialization, session output Z, P, ss and directional keys use protected containers. OPAQUE outputs and Argon2 blocks have zeroizing guards. Consuming transitions drop initial channels and outer/component state. Dependency-private scratch, especially libcrux intermediates, remains a documented erasure limitation; this review does not certify its erasure. |
| Admission | `steps_v2::check_hello` checks bounded canonical shape, profile/method, pairing ID and constant-time MAC before KEM key validation. `PairingWindow::run` joins the listener owner before KEM/authentication. Checking blocks additional admissions; queued streams/listeners are dropped. Failure count is 16. No acceptor survives or reopens a spent window. |
| Outer transcript | `qv-protocol/src/lan_v2.rs` reconstructs complete canonical hello/accept MAC bodies, hashes complete messages and embeds J in T0. `lan_v2.rs` in crypto derives distinct hello/accept keys and epoch/direction keys. B authenticates accept and echoed mode before decapsulation. |
| Password final confirmation | Approver checks committed nonce, obtains local comparison/password approval, verifies KE3, consumes the outer schedule and verifies FinishedB under final keys. Only then does it call `authorized_grant`. B checks KE2 before sending KE3; F and FinishedB are separately framed. No failed-credential decoding distinction is sent to the peer. |
| Grant race and disclosure | `qv-core/src/lan_auth.rs` captures session, scope, exact slot projection and record/version/object/hash; rechecks before KE2 and under the session mutex at a single grant seal. Sequence zero and method/T0 binding are required. No KDF/network/UI wait occurs in that sealing closure. Completed seals report possible disclosure even if cancellation discards their frame. Grant/password-change and cancel-after-seal tests exercise the linearization point. |
| Receiver password check | `LanPasswordGrant::verify` checks bounded descriptor/profiles/UUIDs, unwraps with B's password and constant-time compares VK before bulk receive. `verify_enrollment` later binds the unique staged password head and exact grant identities/projection. B drops its password after the early check, before transfer. |
| Independent VK proof | `KeyAuthentication` separates proof and schedule keys and binds role, method, T0 and A's identity hash. `VerifiedJoiner` can be constructed only after PB verifies. A sends PA and drops epoch zero; `first_state` must authenticate B's first final State before A can send bulk data or final errors. Sync pins known vault/generation; phrase enrollment adopts the proved scope. |
| Record layer and parsing | Closed epoch marker types, consuming `V2Channel::finish`, direction-specific keys/AAD, implicit sequence nonce and 2^32 cap prevent old-key fallback, reflection and replay. Errors poison crypto directions. Frame bounds precede allocation (4096 clear, 4112 credential, 1052672 bulk); protected plaintext is authenticated before message decoding. Strict canonical readers reject trailing/extra/noncanonical data. |
| Exchange and hostile input | States bound scope/features/manifest length; contiguous parts match the announced bounded size. Manifest hash/AEAD/budgets precede object requests. Wants are bounded/deduplicated and restricted to served references. Each object matches ID/profile/length/hash before full archive role/attachment validation. Staging is private and unlinked; no network SQLite ingestion occurs. Progress accounting is now linear. |
| Deadline and lifecycle | Conn carries an absolute cap that message deadlines cannot extend, checks ready I/O too, and polls cancellation in 100 ms slices. A retains its original window expiry; B's enrollment deadline precedes credential entry and candidate attempts. Prompts receive that deadline through the CLI tty/app bridge. Existing-copy core now shares its deadline with local acceptance (finding 1). Fixed KDFs can finish a work unit after expiry but outputs are checked before emission. |
| Commit and publication | `accept_staged` builds and validates a candidate under the existing write transaction with cancellation checks through commit. A committed outcome/index survives a lost Done or pending anchor. Enrollment capability binds private staging scope (including archive ID), method and manifest digest; core preflights before no-replace publication and creates fresh local identity. Post-publication cancellation reports the durable destination rather than claiming nothing happened. |
| Address/discovery policy | Numeric bounded endpoint lists only, no DNS; consistent mapped-address classification. Discovery is disabled, so no advertisements/background retries exist. All-interface listening is explicit. Loopback tests do not establish real two-device operation with Internet blocked. |

The component reference checks used the primary definitions in
[RFC 9807](https://www.rfc-editor.org/rfc/rfc9807.html) and the pinned
[X-Wing draft 11](https://www.ietf.org/archive/id/draft-connolly-cfrg-xwing-kem-11.html).
X-Wing is a KEM; this composition obtains peer authentication separately.
The Ristretto/3DH password route is not claimed to provide post-quantum
authentication merely because final traffic also mixes X-Wing output.

## Evidence and remaining release work

Current commands and results are in PLAN.md. Read tests as regression evidence,
not as proof: wrong-password custom code holders, missing/modified FinishedB,
legacy modes, transcript/role/epoch substitution, proof reflection/replay,
record exhaustion, malformed frames/manifests/requests, exact grant races,
deadline expiry, publication cancellation and commit/anchor crashes are covered.

The existing vectors cross-check canonical encodings/schedules. OPAQUE wire
fields in the composition fixtures are synthetic; the upstream component
fixture uses Identity KSF only for that fixture, while production Stretch is
separately tested. There is still no independent end-to-end implementation of
the complete production Argon2 composition, nor byte-vector coverage of every
error-state transition. This review does not relabel those gaps as completed.

Real two-device LAN-only interruption checks, attended Android and Windows
checks, current target KDF/budget qualification and the independent-party
composition/dependency work remain release evidence. This report closes the
requested agent source review and the two demonstrated code defects. It does
not authorize a release by itself.
