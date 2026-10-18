use serde::Serialize;

/// Fake diagnostics bundle. Phase 3 fills these from the real endpoint.
/// The no-secrets rule applies from day one: never tickets or paths here.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsInfo {
    pub node_id: String,
    pub relay_reachable: bool,
    pub nat_type: String,
}

#[tauri::command]
pub async fn get_diagnostics() -> Result<DiagnosticsInfo, String> {
    Ok(DiagnosticsInfo {
        node_id: "FAKE-NODE-ID-0123456789abcdef".to_string(),
        relay_reachable: true,
        nat_type: "Unknown (fake data)".to_string(),
    })
}
