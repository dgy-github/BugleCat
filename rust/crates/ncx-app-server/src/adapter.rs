/// Runtime side effects that are deliberately outside durable thread storage.
///
/// Desktop, CLI, and future transports implement this boundary instead of
/// matching protocol requests themselves. This keeps request routing owned by
/// the app-server while allowing each host to choose its agent scheduler.
pub trait AppServerAdapter {
    fn create_thread(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), String>;
    fn activate_thread(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), String>;
    fn fork_thread(
        &self,
        source_id: &ncx_protocol::ThreadId,
        target_id: &ncx_protocol::ThreadId,
    ) -> Result<(), String>;
    fn submit_turn(
        &self,
        thread_id: &ncx_protocol::ThreadId,
        text: String,
        images: Vec<String>,
    ) -> Result<(), String>;
    fn interrupt_latest(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), String>;
    fn runtime_status(&self) -> Result<serde_json::Value, String>;
    fn refresh_ready(&self) -> Result<(), String>;
    fn set_workspace(&self, path: String) -> Result<String, String>;
    fn approve(
        &self,
        thread_id: Option<&ncx_protocol::ThreadId>,
        id: u64,
        decision: String,
    ) -> Result<(), String>;
    fn answer(
        &self,
        thread_id: Option<&ncx_protocol::ThreadId>,
        id: u64,
        answer: Option<String>,
    ) -> Result<(), String>;
    fn read_settings(&self) -> Result<serde_json::Value, String>;
    fn update_settings(
        &self,
        updates: std::collections::BTreeMap<String, String>,
    ) -> Result<(), String>;
    fn set_model(&self, model: String) -> Result<(), String>;
    fn set_permission_mode(&self, mode: String) -> Result<(), String>;
    fn read_model_catalog(&self) -> Result<serde_json::Value, String>;
    fn apply_model_preset(
        &self,
        provider_id: String,
        model_id: String,
    ) -> Result<serde_json::Value, String>;
    fn harness_diagnostics(&self) -> Result<serde_json::Value, String>;
    fn list_external_plugins(&self) -> Result<serde_json::Value, String>;
    fn install_external_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String>;
    fn set_external_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), String>;
    fn list_memory(&self) -> Result<serde_json::Value, String>;
    fn add_memory(&self, note: String, tags: Vec<String>) -> Result<bool, String>;
    fn consolidate_memory(&self) -> Result<u64, String>;
    fn list_codex_plugins(&self) -> Result<serde_json::Value, String>;
    fn install_codex_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String>;
    fn set_codex_plugin_enabled(&self, name: String, enabled: bool) -> Result<(), String>;
    fn uninstall_codex_plugin(&self, name: String) -> Result<(), String>;
    fn list_marketplaces(&self) -> Result<serde_json::Value, String>;
    fn install_marketplace_plugin(
        &self,
        marketplace_path: String,
        plugin_name: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String>;
}
