use super::CmdResult;
#[cfg(all(windows, feature = "proxybridge-bundled"))]
use super::StringifyErr as _;
use serde_json::Value;

#[tauri::command]
#[specta::specta]
pub async fn get_application_proxy() -> CmdResult<Option<Value>> {
    #[cfg(all(windows, feature = "proxybridge-bundled"))]
    return crate::application_proxy::view().await.map(Some).stringify_err();
    #[cfg(not(all(windows, feature = "proxybridge-bundled")))]
    Ok(None)
}

#[tauri::command]
#[specta::specta]
pub async fn save_application_proxy(settings: Value) -> CmdResult<Value> {
    #[cfg(all(windows, feature = "proxybridge-bundled"))]
    return crate::application_proxy::save_settings(settings).await.stringify_err();
    #[cfg(not(all(windows, feature = "proxybridge-bundled")))]
    {
        let _ = settings;
        Err("Application proxy is unavailable in this build".into())
    }
}

#[tauri::command]
#[specta::specta]
pub async fn reset_application_proxy_totals() -> CmdResult<Value> {
    #[cfg(all(windows, feature = "proxybridge-bundled"))]
    return crate::application_proxy::reset_totals().await.stringify_err();
    #[cfg(not(all(windows, feature = "proxybridge-bundled")))]
    Err("Application proxy is unavailable in this build".into())
}

#[tauri::command]
#[specta::specta]
pub async fn list_application_proxy_processes() -> CmdResult<Value> {
    #[cfg(all(windows, feature = "proxybridge-bundled"))]
    return crate::application_proxy::processes().await.stringify_err();
    #[cfg(not(all(windows, feature = "proxybridge-bundled")))]
    Err("Application proxy is unavailable in this build".into())
}
