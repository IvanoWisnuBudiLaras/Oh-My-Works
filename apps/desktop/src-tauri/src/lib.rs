use serde::Serialize;
use omw_guardian::{Guardian, HealthLevel};

#[derive(Serialize)]
pub struct MemoryMetricResponse {
    pub status: String,
    pub available_percent: f64,
    pub available_mb: f64,
    pub total_mb: f64,
    pub some_avg10: f64,
    pub full_avg10: f64,
}

#[tauri::command]
fn get_memory_telemetry() -> Result<MemoryMetricResponse, String> {
    match Guardian::assess() {
        Ok((level, p)) => {
            let status = match level {
                HealthLevel::Safe => "SAFE".to_string(),
                HealthLevel::Warning => "WARNING".to_string(),
                HealthLevel::Critical => "CRITICAL".to_string(),
            };
            Ok(MemoryMetricResponse {
                status,
                available_percent: p.available_percent(),
                available_mb: p.available_mem_kb as f64 / 1024.0,
                total_mb: p.total_mem_kb as f64 / 1024.0,
                some_avg10: p.some_avg10,
                full_avg10: p.full_avg10,
            })
        }
        Err(e) => Err(e),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .invoke_handler(tauri::generate_handler![get_memory_telemetry])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
