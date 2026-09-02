/// Runtime side effects that are deliberately outside durable thread storage.
///
/// Desktop, CLI, and future transports implement this boundary instead of
/// matching protocol requests themselves. This keeps request routing owned by
/// the app-server while allowing each host to choose its agent scheduler.
pub trait AppServerAdapter {
    /// Validate a profile against the workspace it will actually run in. Hosts
    /// must not infer this from a process-global current directory because a
    /// desktop workspace switch can race an asynchronous runtime rebuild.
    fn validate_harness_profile(&self, profile: &str, _workspace: &str) -> Result<(), String> {
        if profile == "full" {
            Ok(())
        } else {
            Err(format!("当前宿主不支持 Harness Profile '{profile}'"))
        }
    }

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
        execution_mode: ncx_protocol::ExecutionMode,
    ) -> Result<(), String>;
    fn interrupt_latest(&self, thread_id: &ncx_protocol::ThreadId) -> Result<(), String>;
    /// Schedule the armed persisted Goal for this thread. Hosts must return
    /// only after the request is durably accepted by their local scheduler;
    /// the model call itself remains asynchronous.
    fn continue_goal(
        &self,
        _thread_id: &ncx_protocol::ThreadId,
        _goal: &ncx_protocol::GoalRef,
    ) -> Result<(), String> {
        Err("当前宿主未提供长期目标自动续轮".to_string())
    }
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
    /// Apply a permission-mode rebuild to this durable Thread only. Hosts must
    /// reject it if a later navigation has already made another Thread active.
    fn set_permission_mode(
        &self,
        thread_id: &ncx_protocol::ThreadId,
        mode: String,
    ) -> Result<(), String>;
    fn read_model_catalog(&self) -> Result<serde_json::Value, String>;
    fn apply_model_preset(
        &self,
        provider_id: String,
        model_id: String,
    ) -> Result<serde_json::Value, String>;
    fn list_custom_providers(&self) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供自定义模型商配置".to_string())
    }
    fn save_custom_provider(
        &self,
        _id: Option<String>,
        _name: String,
        _protocol: String,
        _base_url: String,
        _api_key: Option<String>,
        _models: Vec<String>,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供自定义模型商配置".to_string())
    }
    fn delete_custom_provider(&self, _id: String) -> Result<(), String> {
        Err("当前宿主未提供自定义模型商配置".to_string())
    }
    fn discover_custom_provider_models(&self, _id: String) -> Result<Vec<String>, String> {
        Err("当前宿主未提供自定义模型商配置".to_string())
    }
    fn activate_custom_provider(&self, _id: String, _model: String) -> Result<(), String> {
        Err("当前宿主未提供自定义模型商配置".to_string())
    }
    fn probe_custom_provider_chat(
        &self,
        _id: String,
        _model: String,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供模型商对话探测".to_string())
    }
    fn harness_diagnostics(&self) -> Result<serde_json::Value, String>;
    fn list_external_plugins(&self) -> Result<serde_json::Value, String>;
    fn install_external_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String>;
    fn set_external_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), String>;
    fn list_memory(&self, _workspace: String) -> Result<serde_json::Value, String>;
    fn add_memory(
        &self,
        note: String,
        tags: Vec<String>,
        _workspace: String,
    ) -> Result<bool, String>;
    fn consolidate_memory(&self, _workspace: String) -> Result<u64, String>;
    fn start_memory_merge(&self, _workspace: String) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供模型记忆整理".to_string())
    }
    fn memory_merge_status(
        &self,
        _workspace: String,
        _generation: Option<u64>,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供模型记忆整理状态".to_string())
    }
    fn cancel_memory_merge(
        &self,
        _workspace: String,
        _generation: u64,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供模型记忆整理取消".to_string())
    }
    fn forge_runtime_status(&self) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供 Forge 运行时".to_string())
    }
    // Keep Forge's wire fields as separate arguments: this public adapter
    // boundary mirrors `ClientRequest::ForgeJobStart`, and bundling them into
    // a new request type would break existing host implementations. The
    // protocol-level shape is intentional even though Clippy counts it as a
    // large argument list.
    #[allow(clippy::too_many_arguments)]
    fn start_forge_job(
        &self,
        _workspace: String,
        _rounds: u8,
        _repeats: u8,
        _timeout_s: u64,
        _budget_s: u64,
        _teacher: String,
        _accept_margin: u8,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供 Forge 训练任务".to_string())
    }
    fn forge_job_status(
        &self,
        _workspace: String,
        _generation: Option<u64>,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供 Forge 训练状态".to_string())
    }
    fn cancel_forge_job(
        &self,
        _workspace: String,
        _generation: u64,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供 Forge 训练取消".to_string())
    }
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
    fn search_dsh_marketplace(
        &self,
        _source: String,
        _manifest_url: Option<String>,
        _query: String,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供 DSH Marketplace".to_string())
    }
    fn preview_dsh_marketplace_plugin(
        &self,
        _item: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供 DSH Marketplace".to_string())
    }
    fn install_dsh_marketplace_plugin(
        &self,
        _item: serde_json::Value,
        _upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        Err("当前宿主未提供 DSH Marketplace".to_string())
    }
}
