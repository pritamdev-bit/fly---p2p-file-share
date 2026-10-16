# P2P File Transfer (Tauri + iroh) — Research Report

**Subject:** `p2p-file-transfer-plan-tauri.md` (Tauri v2 + React + Rust with `iroh` + `iroh-blobs`)
**Date:** 2026-10-16. **Method:** every claim below was checked against a primary source
(official docs, docs.rs API docs, or upstream source/README). Each finding cites its source.
Anything I could not verify from a primary source is marked **UNVERIFIED**.

**Pinned versions observed on 2026-10-16** (re-check before building):

| Crate | Latest | Source |
|---|---|---|
| `tauri` | 2.12.4 stable (3.0.0-alpha.5 exists; plan correctly targets v2) | [crates.io/api/v1/crates/tauri](https://crates.io/api/v1/crates/tauri) |
| `iroh` | 1.3.0 (2026-09-28) | [docs.rs/iroh](https://docs.rs/iroh/latest/iroh/) |
| `iroh-blobs` | 0.103.1 (2026-10-06), depends on `iroh ^1` | [docs.rs/iroh-blobs](https://docs.rs/iroh-blobs/latest/iroh_blobs/) |
| `sendmer` | 0.11.0 (2026-10-09), uses `iroh ^1` + `iroh-blobs ^0.103` | [crates.io/api/v1/crates/sendmer](https://crates.io/api/v1/crates/sendmer) |
| `wry` (Tauri webview) | 0.57.0 | [wry Cargo.toml](https://raw.githubusercontent.com/tauri-apps/wry/dev/Cargo.toml) |

**Headline findings:** (1) `sendmer` is **AGPL-3.0-only**, single-maintainer — do not copy its
code or depend on the crate in a closed app; use it as read-only reference. (2) Cross-process
**resume requires a persistent store** — the plan's Phase 4 resume test fails with a temp store.
(3) `BlobTicket` carries **no filename** — the plan never says how the filename reaches the
receiver. (4) `iroh` 1.x is now stable, so the plan's "API still moving" warning is outdated;
`iroh-blobs` is still 0.x. (5) Public relays are dev/test-only with rate limits — relay strategy
is a launch decision, not Phase 5 polish.

---

## 1. Improvements

### Architecture

1. **Define filename transport explicitly — it is missing from the plan.** `BlobTicket::new`
   takes only `(EndpointAddr, Hash, BlobFormat)` — "a token containing everything to get a file
   from the provider" — with no filename field
   ([BlobTicket docs](https://docs.rs/iroh-blobs/latest/iroh_blobs/ticket/struct.BlobTicket.html)).
   Filenames must travel inside a **collection** (concatenated 32-byte hashes; first element is
   by convention an app-defined metadata blob), which is how multi-file/name-carrying shares work
   ([blobs protocol doc](https://docs.iroh.computer/protocols/blobs.md)). Follow `sendmer`'s
   collection + metadata approach for v1 single-file too, so the received name is covered by the
   ticket's hash instead of being trusted side-channel chat text.
2. **Use `FsStore`, not `MemStore`, on both sides.** The official blobs walkthrough uses
   `MemStore` and explicitly notes: "There's ways to make this work without having to store the
   whole file in memory! This would involve setting up an `FsStore` instead of a `MemStore`"
   ([blobs doc](https://docs.iroh.computer/protocols/blobs.md)). The plan's "flat memory" claim
   only holds with `FsStore` (+ streaming export). Prefer `ExportMode` accordingly — `sendmer`
   uses `ExportMode::Copy` ([receiver.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/receiver.rs));
   the blobs doc points at `.export_with_opts` with `ExportMode::TryReference` to avoid copies.
3. **Keep the blob `Tag` alive on the sender, not just the node.** Importing returns "a `tag`
   that refers to said blob in the store and allows us to control when/if it gets
   garbage-collected" ([blobs doc](https://docs.iroh.computer/protocols/blobs.md)). The plan's
   "keep the node alive in app state" (Phase 3) is incomplete: store the `Tag` alongside the
   endpoint/router in Tauri managed state or the served blob can be GC'd from a persistent store.
4. **Adopt `sendmer`'s export pipeline as the safety pattern (reimplement, don't copy — see
   V1):** export into a staging dir, validate every path component is `Component::Normal`,
   fail if the target exists (checked via `symlink_metadata`), commit with an atomic no-replace
   rename (`renameat2(RENAME_NOREPLACE)` Linux, `renamex_np(RENAME_EXCL)` macOS, `MoveFileExW`
   without replace flag Windows), and only report success after `ExportProgressItem::Done`
   ([receiver.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/receiver.rs)).
   The plan's `file (1).zip` rename idea is compatible with this if the final commit stays atomic.
5. **Add a timeout design (currently absent).** `sendmer` exposes `--retry-limit` (default 3),
   `--retry-backoff-ms` (250), plus optional connect/metadata/idle timeouts, and documents
   "in-process retry" vs opt-in cross-process resume
   ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)).
   The plan's "sender went offline" message (Phase 4) needs an explicit idle timeout, or a stalled
   transfer hangs forever.
6. **Decide node-identity lifetime now.** `sendmer` generates an ephemeral key per process
   (`PROCESS_SECRET: OnceLock`, random unless `IROH_SECRET` is set)
   ([args.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/args.rs)), so its
   tickets die with the process. Recommend the same default (fresh key per launch): it bounds
   ticket lifetime and avoids a long-term trackable identity. A persistent key enables re-dial by
   `EndpointID` but becomes a tracking identifier — don't persist without a privacy decision.
7. **Offer minimal tickets.** Tickets embed endpoint ID + relay URL + direct addresses and "can
   go stale" as networks change; iroh recommends caching `EndpointID`s and letting lookup resolve
   details for anything long-lived
   ([tickets doc](https://docs.iroh.computer/concepts/tickets.md)).
   `sendmer` supports `--ticket-type id|relay|addresses|RelayAndAddresses`
   ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)). The app
   should default to short `id`-only tickets (less IP exposure, QR-friendly) and fall back to full
   tickets when discovery is unavailable — and fix the plan's "one **short** ticket" wording, since
   iroh's own doc says "Yes, they're long" for full tickets.
8. **Show a sender fingerprint for out-of-band verification.** Endpoint identities are Ed25519
   public keys ([security & privacy doc](https://docs.iroh.computer/concepts/security-privacy.md)).
   Displaying a truncated node-ID fingerprint on both sides lets users confirm over their chat app
   that the ticket wasn't swapped — currently the receiver has zero sender authentication.

### Phases / process

9. **Promote relay strategy from Phase 5 to a pre-launch decision.** Public n0 relays "rate-limit
   traffic… carry no uptime or performance guarantees" and are "suitable for development and
   testing"; production means paid Shared/Dedicated relays or self-hosting
   ([relays doc](https://docs.iroh.computer/concepts/relays.md),
   [dedicated infra](https://docs.iroh.computer/deployment/dedicated-infrastructure.md)). Custom
   relays are one builder call (`RelayMode::Custom`) and a self-host guide exists
   ([dedicated infra](https://docs.iroh.computer/deployment/dedicated-infrastructure.md)). Correct
   the plan's "truly no third-party" framing: relay E2EE means a self-hosted relay gives "control
   of the path", not content visibility
   ([security Q&A](https://docs.iroh.computer/concepts/security-privacy.md)).
10. **Fix the Phase 4 resume test.** "Kill the receiver mid-download, then paste the ticket again"
    only resumes with a **persistent** store: "Retry reuses data already obtained by the current
    receive process. Cross-process resume is opt-in: configure the same private `--cache-dir`"
    ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)). The app
    needs an app-data-dir-backed `FsStore` keyed by content hash, plus TTL/pruning
    (`sendmer` defaults cache TTL to 604800 s with a `cache prune` command). Add "restart app, then
    resume" to the testing matrix.
11. **Add proxy support before the strict-network test.** Without it "the application cannot reach
    a relay at all"; the fix is an application change (`.proxy_from_env()` / `.proxy_url()`), and
    "a proxied deployment usually ends up relaying everything"
    ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md)). The plan's
    "strict office Wi-Fi" test will fail behind proxy-mandating firewalls otherwise.
12. **Decide on UPnP/NAT-PMP port mapping.** It is **on by default** ("asking the gateway to open
    an external port… on by default, and safe to turn off") and disabled via
    `PortmapperConfig::Disabled`
    ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md)). For a
    consumer transfer app, consider disabling it (fewer firewall surprises, no gateway poking) and
    relying on hole-punching + relay; at minimum disclose it.
13. **Bundle diagnostics for support.** `iroh-doctor report` from inside the failing network is
    the documented first diagnostic step, and there is a net-diagnostics guide
    ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md),
    [docs index](https://docs.iroh.computer/llms.txt)). Ship a "copy diagnostics" button (node ID,
    relay reachability, NAT type — never tickets) instead of asking users for logs.
14. **Evaluate n0's `sendme` as the primary reference instead of `sendmer`.** `sendmer` states it
    is "based on [n0-computer/sendme v0.31.0](https://github.com/n0-computer/sendme/commit/6e50167a8a4d45736179cce3d8d5fd803c87c24e)"
    ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)). The
    upstream example is likely more stable and permissively licensed — but I did not verify its
    current state or license (**UNVERIFIED** — check before using).
15. **Pin the Rust toolchain and MSRV in CI.** The dialog plugin already "requires a Rust version
    of at least **1.90**" ([dialog plugin](https://tauri.app/plugin/dialog/)). With `cargo`
    installed per Tauri prerequisites, add `rust-toolchain.toml` + `cargo --locked` builds.

### Packaging / UX details

16. **WebView2 choice is correctly framed; record the numbers.** Default `downloadBootstrapper`
    costs 0 MB but needs internet; `embedBootstrapper` ≈ 1.8 MB; `offlineInstaller` ≈ 127 MB;
    `fixedVersion` ≈ 180 MB; `skip` is not recommended. Win10 (April 2018+) and Win11 ship the
    runtime with the OS ([Windows installer doc](https://tauri.app/distribute/windows-installer/)).
    Keep the default and document the offline-installer tradeoff.
17. **Build MSI only on Windows; use per-OS CI runners as planned.** "`.msi` installers can
    **only be created on Windows**"; NSIS cross-compilation "should only be used as a last resort"
    ([Windows installer doc](https://tauri.app/distribute/windows-installer/)). The plan's
    GitHub-Actions-per-OS approach is correct — don't try to cross-build MSI from Linux.
18. **Copy-ticket UX:** Tauri has a first-party clipboard plugin
    ([plugin index](https://tauri.app/plugin/dialog/) lists `/plugin/clipboard/`), and `sendmer`
    ships a `--clipboard` flag plus `TransferEvent`-driven UI updates
    ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)). Use
    the Tauri plugin rather than a JS clipboard hack (works outside the webview sandbox).
19. **Sign releases with Sigstore like the reference does.** `sendmer`'s installers "verify the
    archive and SBOM with Sigstore" via `cosign`
    ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)).
    Adopt Sigstore + SBOM for the app's own GitHub releases; it is independent of OS code signing.
20. **Consider mDNS for same-network transfers.** iroh documents mDNS address lookup for the local
    network ([docs index](https://docs.iroh.computer/llms.txt)). Same-room transfers could skip
    public discovery entirely — a later enhancement, deliberately off by default (see V14).

---

## 2. Issues (correctness, UX, cross-OS, NAT, resume, large files)

1. **"One short ticket" is inaccurate.** Full tickets pack endpoint ID + relay URL + direct
   addresses as base32 postcard data; iroh's doc shows a ~200-char example and says "Yes, they're
   long" ([tickets doc](https://docs.iroh.computer/concepts/tickets.md)). Plan the UI for long
   tickets (copy button, QR) rather than "short" human-transcribable codes; `id`-only tickets are
   the short option (see improvement 7).
2. **Tickets go stale; long "waiting for receiver" sessions can break.** "The dialing information
   in a ticket (especially IP addresses) can become outdated as network conditions change"
   ([tickets doc](https://docs.iroh.computer/concepts/tickets.md)). If the sender's network
   changes (laptop sleep/wake, DHCP, hotspot toggle) while waiting, an old ticket may fail even
   though both apps are open. Mitigate with `id`-only tickets + live discovery, and regenerate the
   displayed ticket when the endpoint's addresses change.
3. **The "API still moving" warning is outdated for `iroh`, still true for `iroh-blobs`.** `iroh`
   is at 1.3.0 — a semver-stable major line with `Endpoint::bind()` / `Endpoint::builder(...)`
   ([docs.rs/iroh 1.3.0](https://docs.rs/iroh/latest/iroh/), [iroh README](https://raw.githubusercontent.com/n0-computer/iroh/main/README.md)).
   `iroh-blobs` is still 0.x (0.103.1), i.e. no stability guarantee — keep the pin-exact-versions
   discipline for it, and note the README caveat quoted in issue 4.
4. **`iroh-blobs` README carries a production-quality caveat.** "NOTE: this version of iroh-blobs
   is not yet considered production quality. For now, if you need production quality, use
   iroh-blobs 0.35" ([iroh-blobs README](https://raw.githubusercontent.com/n0-computer/iroh-blobs/main/README.md)).
   It is unclear which versions this note covers (current release is 0.103.1), so treat it as a
   flag to resolve with maintainers/testing, not a blocker — but do not skip soak-testing large
   transfers before release.
5. **Plan's capability model description is imprecise.** Tauri v2 denies *plugin* permissions by
   default, but "all commands that you registered in your app… are allowed to be used by all the
   windows and webviews of the app" unless restricted via `AppManifest::commands` in `build.rs`
   ([capabilities doc](https://tauri.app/security/capabilities/)). The checklist item "grant only
   the capabilities you need (dialog, your two commands)" conflates the two mechanisms: the dialog
   plugin needs `dialog:allow-open` / `dialog:allow-save`
   ([dialog permissions](https://tauri.app/plugin/dialog/)); the app's own commands need no
   capability entries in a single-window app.
6. **Disk-space check must budget ~2x the file size with copy-based export.** `sendmer` exports
   with `ExportMode::Copy` into staging, then renames into place
   ([receiver.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/receiver.rs)),
   so peak usage is blob-store copy + final file (plus ~6% BLAKE3 outboard metadata at default 1
   KiB chunks — [blobs doc](https://docs.iroh.computer/protocols/blobs.md)). Either check for 2x
   free space or use `ExportMode::TryReference` ([blobs doc](https://docs.iroh.computer/protocols/blobs.md)).
7. **Silent-corruption scope: integrity covers content under the ticket hash, nothing else.**
   Verified streaming checks chunks against the BLAKE3 tree per §6.4 of the BLAKE3 paper
   ([blobs doc](https://docs.iroh.computer/protocols/blobs.md);
   [BLAKE3 README](https://raw.githubusercontent.com/BLAKE3-team/BLAKE3/master/README.md)). Any
   filename/metadata sent *outside* the ticket's hash (e.g. typed into chat next to the ticket)
   is unauthenticated — another reason filenames must travel in the hashed collection (improvement 1).
8. **Relay is always in the loop, even for "direct" transfers.** Each device holds "long-lived
   encrypted connections to relay servers on TCP port 443… It is always used, at minimum for the
   introduction" ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md)).
   Consequences: (a) relay reachability on 443 is a hard requirement, test it; (b) relay sees
   connection metadata even when data goes direct (see V6); (c) "no relay" testing needs
   `RelayMode::Disabled`, not just hoping for direct.
9. **DNS-based discovery has enterprise failure modes.** iroh finds devices via DNS TXT records
   under `dns.iroh.link`; "some DNS security products block or strip TXT records" and split
   behavior (works with a full ticket, fails by identity) indicates a DNS problem
   ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md)). Add a "works
   with full ticket, fails with short ticket → DNS filtering" branch to error messages.
10. **Firewall idle timeouts can silently kill long transfers.** "A UDP idle timeout of 30 seconds
    or more on your firewall is recommended" and iroh gives up on idle paths after a timeout
    ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md)). Slow/large
    transfers over stateful firewalls need keepalive behavior verified — add a throttled-network
    soak test to the matrix.
11. **Windows firewall prompt on first run is expected.** "Devices do listen on the UDP sockets
    they bind" ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md)), so
    Windows will prompt. The plan's Phase 6 item is correct — extend it with in-app guidance text
    for users who clicked "Cancel" (transfer will be relay-only/slower, or fail if UDP-blocked).
12. **Linux runtime dependency is structural, not just build-time.** `wry` 0.57.0 links
    `webkit2gtk`, `javascriptcore-rs`, `gtk`, `soup3` system libraries on Linux and `webview2-com`
    on Windows ([wry Cargo.toml](https://raw.githubusercontent.com/tauri-apps/wry/dev/Cargo.toml));
    dev prerequisites are `libwebkit2gtk-4.1-dev` et al.
    ([prerequisites](https://tauri.app/start/prerequisites/)). The plan's Phase 0 Linux gate and
    clean-machine `.deb`/AppImage tests are the right mitigation — keep them, and test on a distro
    generation newer *and* older than the build host (webkit2gtk 4.0 vs 4.1 soname split).
13. **No update mechanism is planned.** Tauri's updater *requires* signed updates ("needs a
    signature… This cannot be disabled"), with keys from `tauri signer generate`, pubkey in
    `tauri.conf.json`, private key via `TAURI_SIGNING_PRIVATE_KEY`
    ([updater plugin](https://tauri.app/plugin/updater/)). Without an updater, every security fix
    depends on users manually reinstalling — plan the update channel (or an explicit decision
    against it) before v1 ships.
14. **Laptop sleep will stall transfers; no wake handling is planned.** (General engineering note,
    **UNVERIFIED** against a primary source.) QUIC connections do not survive sleep; on wake the
    session must re-establish and resume from the persistent store. The "keep this window open"
    message should say "keep this device awake", and resume-after-sleep belongs in the test matrix.
15. **The "WebKitGTK has WebRTC disabled" rationale is background, not verified.** The plan's
    motivation for Rust-side networking cites WebKitGTK WebRTC defaults — I did not verify this
    against wry/WebKitGTK sources (**UNVERIFIED**). It doesn't affect the build either way (the
    Rust-networking decision stands on its other merits: IPC cost, installer size, resume library).

## 3. Vulnerabilities and hardening gaps

- **V1. AGPL-3.0 contamination from `sendmer` (license).** `sendmer` is licensed
  `AGPL-3.0-only` ([crates.io metadata](https://crates.io/api/v1/crates/sendmer),
  [sendmer README §License](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)), by a
  single maintainer, 493 total downloads, first published 2025-12-24. Copying its code *or*
  depending on the crate in a distributed closed-source app triggers AGPL obligations (this is the
  standard FSF interpretation — get legal confirmation, don't take this report as legal advice).
  The plan's "copy proven patterns from `sendmer`" must become **read-only reference +
  clean-room reimplementation** (its export pipeline and event taxonomy are still the right model
  — see improvements 4–5). Also note its API keeps churning (plan cites 0.2.x→0.3.x renames;
  current 0.11.0 prefers `SendHandle` with legacy shims).
- **V2. Tickets are reusable bearer capabilities.** "Tickets are reusable… can be used multiple
  times by anyone who has it", and "sharing a ticket means sharing your IP address"
  ([tickets doc](https://docs.iroh.computer/concepts/tickets.md)); a ticket stays valid "while its
  sender router is alive" and cancelling stops new receives
  ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)). The plan
  covers "treat tickets as secrets" but should add: explicit **Stop sharing** that closes the
  router, auto-stop after first completed download (v1 = one receiver), optional session lifetime
  (`sendmer` supports `--session-lifetime-seconds`), and UI wording that forwarding the ticket =
  granting the download.
- **V3. Path traversal via remote filenames.** `sendmer` defends by rejecting any path whose
  components aren't all `Component::Normal` (no `..`, absolute paths, or prefixes) and refusing to
  materialize symlinks/unsafe paths
  ([receiver.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/receiver.rs)).
  The plan's "sanitize the filename" item must be implemented at least this strictly — never
  `join()` a remote name onto the save dir without validation.
- **V4. Destructive overwrite / TOCTOU on commit.** `sendmer`'s policy is fail-if-exists with
  atomic no-replace renames per OS, precisely to beat races between check and commit
  ([receiver.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/receiver.rs)).
  The plan's `file (1).zip` auto-rename is fine UX but the final commit must still use a no-replace
  primitive (or `create_new`-style exclusive create) — check-then-rename in two steps is racy.
  OS-specific extras (Windows reserved names, ADS colons, trailing dots, non-UTF8 names) still need
  a rules pass (**UNVERIFIED** against a primary source — verify against platform docs during Phase 4).
- **V5. Symlink attacks.** `sendmer` rejects symlinks in legacy collection mode and checks
  `symlink_metadata` (not following links) before commit
  ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md),
  [receiver.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/receiver.rs)).
  Never materialize a symlink from remote metadata, and never follow a pre-existing symlink at the
  destination during commit.
- **V6. Relay/discovery metadata exposure.** Relays "cannot read" content but see "the identities
  of the devices connected to it, which pairs are exchanging traffic, when, and how much"
  ([security Q&A](https://docs.iroh.computer/concepts/security-privacy.md)); endpoints
  "automatically publish their own addressing information" (home relay, possibly direct addresses)
  to lookup services ([address lookup doc](https://docs.iroh.computer/concepts/address-lookup.md));
  public relays have "no guarantees around privacy or uptime" and are not recommended for
  production ([security doc](https://docs.iroh.computer/concepts/security-privacy.md)). Disclose
  this in-app (a "how private is this?" note), and treat self-hosting as the mitigation (improvement 9).
- **V7. UPnP opens inbound ports by default.** iroh "asks your gateway for an external port over
  UPnP or NAT-PMP" by default, including multicasting discovery to `239.255.255.250:1900`
  ([enterprise networks doc](https://docs.iroh.computer/configuring-networks.md)). A file-transfer
  app silently reconfiguring the user's gateway is a hardening finding — disable or ask (improvement 12).
- **V8. Over-broad Tauri surface.** Keep the capability set minimal: `dialog:allow-open` and
  `dialog:allow-save` only (message dialogs need `dialog:allow-message`; `ask`/`confirm` are
  deprecated aliases) ([dialog permissions](https://tauri.app/plugin/dialog/)). Do **not** add the
  fs/opener/shell plugins — all file IO stays in Rust behind the two commands, and received files
  must never be auto-opened or previewed in the webview (extends the plan's item correctly). If a
  second window is ever added, scope app commands with `AppManifest::commands`
  ([capabilities doc](https://tauri.app/security/capabilities/)).
- **V9. Secret leakage through logs, diagnostics, and decoders.** `sendmer` keeps "failure and
  cleanup diagnostics" free of "source/destination paths, tickets, or lower-level connection text"
  ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)) — apply
  the same rule to app logs, crash reports, and the diagnostics bundle (improvement 13). Separately,
  iroh's docs invite users to paste tickets into the hosted
  [ticket explorer](https://ticket.iroh.computer)
  ([tickets doc](https://docs.iroh.computer/concepts/tickets.md)) — warn users that pasting a live
  ticket into any website hands its bearer capability (and sender IPs) to that site; ship an
  in-app ticket inspector instead.
- **V10. No sender authentication (ticket swapping).** Nothing binds a ticket to the chat contact
  who sent it. Mitigate with the fingerprint-confirmation UX (improvement 8); document that the
  chat channel is part of the trust base.
- **V11. Resource-exhaustion abuse.** Anyone holding a ticket can download; large files cost the
  sender bandwidth and the receiver ~2x disk during copy-export (see issue 6). Adopt `sendmer`'s
  server-side guards as options: `--max-receivers`, `--max-upload-rate`, `--session-lifetime`,
  `--max-total-size` ([sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md)).
  v1 should at least cap concurrent receivers at 1 and stop serving on completion.
- **V12. Unsigned releases + no update channel.** The plan covers OS signing/SmartScreen; add:
  key management for the Tauri updater signing key ("if you lose this key you will NOT be able to
  publish new updates") ([updater plugin](https://tauri.app/plugin/updater/)), plus Sigstore/SBOM
  on GitHub releases (improvement 19). Until an updater exists, version checks should at minimum
  point users at a download page — never silently run outdated crypto.
- **V13. Supply-chain pinning.** Pin `iroh`/`iroh-blobs` exactly with a committed `Cargo.lock`
  (the plan says this — keep it), run `cargo audit` in CI (plan says this too), and note the
  asymmetric risk: `iroh` 1.x is semver-stable, `iroh-blobs` 0.x is not
  ([docs.rs](https://docs.rs/iroh-blobs/latest/iroh_blobs/)). Review the `iroh-blobs` changelog on
  every bump; do not float `^0.103`.
- **V14. DHT/mDNS must stay off unless deliberately chosen.** iroh offers DHT lookup "through the
  BitTorrent Mainline DHT" and mDNS on the LAN
  ([address lookup doc](https://docs.iroh.computer/concepts/address-lookup.md)). DHT would
  broadcast transfer participation to a public swarm — for a private-transfer app, use only the N0
  preset's DNS/Pkarr + ticket-embedded addresses (and document that choice).
- **V15. Clipboard as ticket channel.** Tickets via clipboard are readable by any app watching it
  (general platform behavior, **UNVERIFIED** against a primary source). Prefer QR display +
  in-app paste, auto-clear the ticket display after use, and never log clipboard contents.

## 4. Sources and verification notes

**Tauri (official docs + source, v2 unless noted)**

- Commands (unique names, camelCase args, `Serialize` incl. errors, single `generate_handler!`):
  [tauri.app/develop/calling-rust](https://tauri.app/develop/calling-rust/) — plan's Phase 2 sketch
  is consistent with all of these.
- Channels (fast, ordered, `tauri::ipc::Channel<T>` + frontend `Channel`/`onmessage`, recommended
  over events for progress): [tauri.app/develop/calling-frontend](https://tauri.app/develop/calling-frontend/).
- Events are unordered under rapid async emission — use channels instead (same page). Do not use
  events for progress.
- State (`app.manage`, `Mutex` interior mutability, async-mutex guidance, no `Arc` needed, cheap
  `AppHandle` clone): [tauri.app/develop/state-management](https://tauri.app/develop/state-management/).
- Capabilities (app commands allowed-by-default; `AppManifest::commands` to scope; platform
  targeting): [tauri.app/security/capabilities](https://tauri.app/security/capabilities/).
- Dialog plugin (npm `@tauri-apps/plugin-dialog` + `tauri-plugin-dialog`, `init()`, permission
  table, Rust ≥ 1.90): [tauri.app/plugin/dialog](https://tauri.app/plugin/dialog/).
- Updater (mandatory signatures, `signer generate`, env key handling):
  [tauri.app/plugin/updater](https://tauri.app/plugin/updater/).
- Linux prerequisites (`libwebkit2gtk-4.1-dev` etc.):
  [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/).
- Windows installer (MSI Windows-only, NSIS cross-compile last resort, WebView2 mode table):
  [tauri.app/distribute/windows-installer](https://tauri.app/distribute/windows-installer/).
- `wry` 0.57.0 system linkage (webkit2gtk/gtk/soup3 Linux, webview2-com Windows):
  [wry Cargo.toml](https://raw.githubusercontent.com/tauri-apps/wry/dev/Cargo.toml).
- Crate version: [crates.io/api/v1/crates/tauri](https://crates.io/api/v1/crates/tauri).

**iroh (official docs + docs.rs + source)**

- `Endpoint::bind()` / builder + `Router` echo example, QUIC via `noq` with authenticated
  encryption: [iroh README](https://raw.githubusercontent.com/n0-computer/iroh/main/README.md);
  crate 1.3.0: [docs.rs/iroh](https://docs.rs/iroh/latest/iroh/).
- Presets (`N0` = DNS/Pkarr lookup + n0 relays via `iroh.link`; `Minimal`; `N0DisableRelay`):
  [presets::N0 docs](https://docs.rs/iroh/latest/iroh/endpoint/presets/struct.N0.html).
- Relays (E2EE, metadata-visible, always used for introduction, ~9/10 direct, public = dev/test,
  rate-limited, no SLA): [concepts/relays](https://docs.iroh.computer/concepts/relays),
  [security & privacy](https://docs.iroh.computer/concepts/security-privacy.md).
- Tickets (contents, reusable, IP-sharing, stale, postcard/base32 format, explorer link):
  [concepts/tickets](https://docs.iroh.computer/concepts/tickets.md).
- Address lookup (auto-publish, Pkarr+DNS, mDNS/DHT options):
  [concepts/address-lookup](https://docs.iroh.computer/concepts/address-lookup.md).
- Firewall/proxy/UPnP/DNS-TXT/UDP-idle-timeout/`iroh-doctor`:
  [configuring-networks](https://docs.iroh.computer/configuring-networks.md).
- Custom relays (`RelayMode::Custom`), self-host guide, managed/shared tiers:
  [dedicated-infrastructure](https://docs.iroh.computer/deployment/dedicated-infrastructure.md).
- Page index used for discovery: [llms.txt](https://docs.iroh.computer/llms.txt).

**iroh-blobs**

- Crate 0.103.1 + `iroh ^1` dependency: [docs.rs](https://docs.rs/iroh-blobs/latest/iroh_blobs/).
- `BlobTicket` shape and docs: [ticket struct](https://docs.rs/iroh-blobs/latest/iroh_blobs/ticket/struct.BlobTicket.html).
- Protocol (verified streaming per BLAKE3 §6.4, 1 KiB chunks, 6% overhead, outboard, ranges,
  collections, tags/GC, `MemStore` vs `FsStore`, `ExportMode::TryReference`):
  [protocols/blobs](https://docs.iroh.computer/protocols/blobs.md),
  [iroh-blobs README](https://raw.githubusercontent.com/n0-computer/iroh-blobs/main/README.md)
  (includes the production-quality caveat quoted in issue 4).

**sendmer (reference implementation — read-only, AGPL)**

- Metadata (0.11.0, AGPL-3.0-only, single maintainer, deps `iroh ^1`/`iroh-blobs ^0.103`):
  [crates.io/api/v1/crates/sendmer](https://crates.io/api/v1/crates/sendmer),
  [0.11.0 dependencies](https://crates.io/api/v1/crates/sendmer/0.11.0/dependencies).
- Behavior (temp stores, opt-in `--cache-dir` resume, fail-only conflicts, ticket types,
  retry/timeouts, sender caps, `SendHandle` API, no-secret diagnostics, Sigstore releases,
  sendme-v0.31.0 attribution): [sendmer README](https://raw.githubusercontent.com/bruceblink/sendmer/main/README.md).
- Export safety (staging, `Component::Normal`, `symlink_metadata`, per-OS no-replace rename,
  `ExportMode::Copy`, `GetProgressItem::{Progress,Done,Error}`):
  [receiver.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/receiver.rs).
- Endpoint setup (`presets::Minimal` + relay mode + ALPNs, ephemeral secret):
  [endpoint.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/endpoint.rs),
  [args.rs](https://raw.githubusercontent.com/bruceblink/sendmer/main/src/core/args.rs).

**BLAKE3**

- Merkle tree, verified streaming, 256-bit default output, multithreading notes:
  [BLAKE3 README](https://raw.githubusercontent.com/BLAKE3-team/BLAKE3/master/README.md).

**Could not verify from a primary source (UNVERIFIED)**

- WebKitGTK WebRTC-disabled-by-default (plan background claim; irrelevant to the build).
- Scope of the iroh-blobs "use 0.35 for production" note (quote verified; version coverage unclear).
- n0-computer/sendme current state/license as an alternative reference.
- Windows reserved-name/ADS/trailing-dot filename rules (verify against platform docs in Phase 4).
- Laptop-sleep transfer behavior (engineering expectation; needs a test, not a citation).
- Clipboard-snooping exposure (general platform behavior).

