use crate::share::build_endpoint;
use iroh::Watcher;
use serde::Serialize;
use std::time::Duration;

/// Real diagnostics bundle. The no-secrets rule: never tickets or paths.
/// The node ID is a fresh ephemeral identity per call — fly keeps no
/// long-term identity, so it differs every time by design.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsInfo {
    pub node_id: String,
    pub relay_reachable: bool,
    pub nat_type: String,
}

const ONLINE_TIMEOUT: Duration = Duration::from_secs(15);
const REPORT_TIMEOUT: Duration = Duration::from_secs(10);

#[tauri::command]
pub async fn get_diagnostics() -> Result<DiagnosticsInfo, String> {
    let endpoint = build_endpoint()
        .await
        .map_err(|error| format!("{error:#}"))?;
    let node_id = endpoint.id().to_string();
    // online() resolves once any home relay connects.
    let relay_reachable = tokio::time::timeout(ONLINE_TIMEOUT, endpoint.online())
        .await
        .is_ok();
    let nat_type = match tokio::time::timeout(
        REPORT_TIMEOUT,
        endpoint.net_report().initialized(),
    )
    .await
    {
        Ok(report) => classify_nat(&report),
        Err(_) => "Unknown".to_string(),
    };
    endpoint.close().await;
    Ok(DiagnosticsInfo {
        node_id,
        relay_reachable,
        nat_type,
    })
}

/// Human NAT summary from a net report. There is no direct "NAT type"
/// field; mapping stability plus UDP reachability is what can be observed.
fn classify_nat(report: &iroh::unstable_net_report::NetReport) -> String {
    if !report.has_udp() {
        return "UDP blocked".to_string();
    }
    match report.mapping_varies_by_dest() {
        Some(true) => "Symmetric NAT".to_string(),
        _ => "Cone NAT".to_string(),
    }
}
