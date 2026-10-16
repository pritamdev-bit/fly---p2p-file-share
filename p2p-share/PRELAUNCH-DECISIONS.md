# Pre-launch decisions

Started in Phase 0 per `../p2p-file-transfer-plan-tauri.md`. Each item is mostly
one-line config, but all are harder to change after ship. Status: PROPOSED unless
marked DECIDED.

| # | Decision | Plan default / proposal | Status |
|---|---|---|---|
| 1 | Relay tier | Paid Shared/Dedicated or self-hosted (`RelayMode::Custom`). Public n0 relays are dev/test-only (rate-limited, no SLA/privacy). | PROPOSED |
| 2 | Updater channel | Ship Tauri updater (signatures mandatory; guard `TAURI_SIGNING_PRIVATE_KEY`) or record an explicit no. Until one exists, version checks point at a download page. | PROPOSED |
| 3 | Node identity lifetime | Ephemeral key per launch (bounds ticket lifetime, no long-term trackable identity). | PROPOSED |
| 4 | UPnP/NAT-PMP port mapping | Disabled (rely on hole-punching + relay) — or disclose that the app asks the gateway for a port. | PROPOSED |
| 5 | DHT / mDNS lookup | Off. Use the N0 preset (DNS/Pkarr + ticket-embedded addresses) only; document the choice. | PROPOSED |
| 6 | Receiver / session caps | Max 1 concurrent receiver in v1; auto-stop serving after first completed download; consider session-lifetime option. | PROPOSED |

References: plan sections 4, 6, 9 and `../p2p-file-transfer-research.md` (I6, I9, I12, V2, V6, V7, V11, V12, V14).
