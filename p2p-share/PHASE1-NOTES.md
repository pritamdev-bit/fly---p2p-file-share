# Phase 1 notes (sendmer 0.11.0, observed 2026-10-17)

How to read this: behavior notes in our own words for Phases 3/4. The
referenceRepo was read from a scratch clone outside this project and nothing
was copied — `sendmer` is AGPL-3.0-only (LICENSE file in its repo root).

## 1. Ticket anatomy (measured, not quoted)

- Real ticket format on the wire is `blob` + payload with **no colon**
  (the README example showing `blob:...` is stale).
- Lengths for the same 13-byte file: `id` = **113 chars**,
  `RelayAndAddresses` (default) = **209 chars** (~1.85x).
- Ticket-type flag: `--ticket-type id|relay|addresses|RelayAndAddresses`
  (combined mode needs the exact `RelayAndAddresses` spelling).
- Contents: content hash + sender endpoint ID + (depending on type) relay
  URL and/or direct IP addresses. Reusable bearer capability: valid while
  the sender router is alive, dead afterwards (verified: every probe used
  `--session-lifetime-seconds`, senders exited, tickets expired).
- Ephemeral identity confirmed two ways: (a) source generates a random key
  per process unless `IROH_SECRET` is set; (b) 7 probe runs produced 7
  distinct endpoint IDs. This matches our planned default.

## 2. Resume semantics (the Phase 4 bar)

- Receiver uses a **temporary store by default**. Kill + re-run with no
  flags = **full re-download**.
- Same-process retry reuses already-obtained data: `--retry-limit 3`,
  `--retry-backoff-ms 250` (defaults).
- Cross-process resume is **opt-in**: same `--cache-dir` for the later
  `receive` of the same content. Cache is content-addressed, entries carry
  a TTL (default 7 days / 604800 s), expired entries auto-prune on open,
  `sendmer cache prune --cache-dir <path>` does it explicitly, and a
  **successful export removes the completed entry**.
- Either way, the sender must still be reachable.

## 3. Export pipeline (spec to reimplement, not code)

1. Collections must have exactly **one top-level root**; empty or
   multi-root collections are rejected (fail, never partial commit).
2. Every remote path component must be a plain relative segment
   (`Component::Normal`); anything else rejects the transfer.
3. Export goes to a **unique staging dir inside the output dir**
   (timestamp + attempt counter, up to 100 tries), via
   `export_with_opts` in `Copy` mode. A blob counts as exported **only
   after its progress stream yields `Done`**; a stream that closes
   without `Done`, or yields `Error`, fails the receive.
4. Pre-commit: if the final target exists (checked without following
   symlinks), fail with a target-conflict error. Policy is **fail-only**:
   never merge, rename, skip, or overwrite.
5. Commit with an **atomic no-replace move**: Linux `renameat2` with
   `RENAME_NOREPLACE`, macOS `renamex_np` with `RENAME_EXCL`, Windows
   `MoveFileExW` with flags `0`. An `AlreadyExists` from the move maps
   back to target-conflict (beats check-then-act races).
6. Staging is cleaned on every failure path. The move is the commit
   point: post-move cleanup trouble must not turn success into failure.
7. Filenames: sender uses the last path component as the shared name;
   symlinks are always rejected; legacy collections can't preserve empty
   dirs (opt-in TM1 `--manifest` mode handles those + portable metadata).

## 4. Timeouts, caps, events (for our TransferEvent design)

- All three timeouts exist and are **unset by default**: per-attempt
  connect timeout, collection-metadata timeout, download idle timeout
  (no stream progress). Our "sender went offline" needs the idle one or
  a stall hangs forever.
- Optional sender caps: shared total upload rate (payload only, no
  overhead), max concurrent receivers (a disconnect frees its slot),
  max files, max total size, import working-set memory budget, and a
  fixed session lifetime counted from ready (zero rejected, receiver
  activity does not extend it).
- Event model worth mirroring: one random session id, strictly
  increasing sequence numbers, explicit phase per event
  (preparing/metadata/exporting/finalizing), at most one terminal event
  (completed/failed/cancelled), and structured failures (error code +
  phase + retryable flag + safe message). Consumers match on the event
  type and never parse error text.
- Failure taxonomy: bad ticket = invalid input; unreachable sender /
  dropped connection = retryable transfer-interrupted; sender actively
  refusing = non-retryable remote-rejected; existing destination =
  non-retryable target-conflict.
- Diagnostics rule: logs carry codes/phases/attempt counts only — never
  tickets, file paths, or connection text. Cancellation (API or Ctrl+C)
  closes endpoint + store + temp dir on one cleanup path.

## 5. Endpoint construction notes

- Builder uses the `Minimal` preset plus explicit ALPNs, the process
  secret, and a relay mode (default = standard relay map). Our plan says
  the N0 preset — equivalent outcome here, revisit in Phase 3.
- Both sides accept fixed UDP listen addresses for firewall rules
  (`--magic-ipv4-addr` / `--magic-ipv6-addr`); default is a random free
  port each run.
- Relay flag accepts `default`, `disabled`, or a URL — useful for the
  no-relay and relay-only matrix rows.

## 6. This-network observations (this PC, 2026-10-17)

- Home relay auto-selected: `aps1-1.relay.n0.iroh.link` (region-aware).
- Default relays reachable on TCP 443: `use1-1` (87.99.151.110) and
  `euc1-1` (91.99.237.97) both answered.
- `dns.iroh.link` resolves (A + AAAA) — discovery DNS works here.
- Every run logged `IPv4 address detected by QAD varies by destination`
  — this NAT maps different external addresses per destination
  (address-dependent mapping), so hole punching may be harder from here
  and the relay fallback matters. The two-PC test is the real verdict.
- `iroh-doctor` (crate v0.101.0) does **not** compile on rustc 1.99
  (`ssh-key` errors), and the `iroh` 1.3.0 crate ships no binaries, so
  the documented `iroh-doctor report` step was replaced with the direct
  probes above. Retry with an older toolchain only if the transfer test
  produces a network mystery.

## 7. Two-PC run results (2026-10-17, home WiFi vs phone hotspot)

- [x] 1 GB transfer, default ticket: **works**. 11:26:10 → 11:52:23
      (~26 min, ~0.68 MiB/s), byte count verified
      (`Downloaded 1 files, 1073741824 bytes`). Very likely relayed
      (see §8); no explicit direct/relay line at default verbosity.
- [ ] Same with `--ticket-type id`: **failed once** with receiver
      `decoding error` ~23 s in (`attempt failed attempt=1`); sender
      side unknown (sender log empty). Most likely the sender died and
      the receiver reported the truncated stream — not yet proven.
      A second id-ticket run (also id) started cleanly, so not
      systematically broken. Optional rerun to classify flaky vs real.
- [x] Resume with `--cache-dir`: **works** (user-observed; log shows
      `reconstructed range is RangeSet{0..321744}` = verified ranges
      reloaded cross-process). Without-cache contrast run not logged.
- [ ] Kill the sender mid-transfer: not cleanly captured (see the
      id-ticket failure above — may itself be this case).
- [ ] Firewall prompt wording: not reported.
- [ ] Ticket staleness: not tested (optional).

## 8. What the receiver logs teach our app

- **Relayed, slow, but bulletproof.** The 1 GiB run survived a home-relay
  switch (euc1-1 → aps1-1), a relay ping-timeout disconnect, repeated
  Windows UDP `sendmsg` errors (10040/10022/10051), IPv6 route flapping,
  and captive-portal probe timeouts — and still completed with the exact
  byte count. None of this transport noise may ever reach our users as
  errors: calm progress bar + friendly failure messages only, raw detail
  confined to the diagnostics bundle.
- **Speed reality check.** ~0.68 MiB/s over hotspot relay means the
  10–20 GB matrix row takes hours on such paths. Speed + ETA display
  (planned) and resume-as-core-feature (planned) are the correct
  conclusions; nothing new needed.
- **Sender "done" must be explicit.** Observed: the sender sat at
  1023/1024 MB after the receiver had fully completed. Served-bytes
  never reliably equals done (sendmer has no sender-side completion
  state — it serves until Ctrl+C). Our sender status must treat
  **receiver completion as the source of truth**, plus idle detection
  and auto-stop — never `bytes_served == file_size`.
- **Sender logs need `--json-events`.** `sendlogs.txt` came back empty:
  stdout is block-buffered when redirected and Ctrl+C discards the
  buffer. For future sender-side captures, use `--json-events` (flushed
  line-delimited events) piped to a file.
- **id tickets: one unexplained failure.** Only open question from this
  phase. If it matters before Phase 3, rerun one id-ticket transfer
  with `--json-events` logging on both sides; a fast failure keeps it
  cheap.
