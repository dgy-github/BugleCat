use super::*;
use ncx_protocol::{ClientRequest, ItemId, ResponsePayload, ThreadId, ThreadItem, TurnId};
use ncx_thread_store::JsonThreadStore;
use std::sync::atomic::AtomicU64;
use std::sync::Mutex;

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn server() -> AppServer<JsonThreadStore> {
    let path = std::env::temp_dir().join(format!(
        "ncx-app-server-{}-{}.json",
        std::process::id(),
        TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&path);
    AppServer::new(Arc::new(JsonThreadStore::open(path).unwrap()), || 100)
}

mod goal_driver_tests;
mod runtime_tests;
mod thread_tests;
#[derive(Default)]
struct RecordingRuntime {
    calls: Mutex<Vec<String>>,
    fail_goal_continue: bool,
}

impl AppServerAdapter for RecordingRuntime {
    fn validate_harness_profile(&self, profile: &str) -> Result<(), String> {
        matches!(
            profile,
            "full" | "coding" | "readonly" | "minimal" | "headless"
        )
        .then_some(())
        .ok_or_else(|| format!("unknown Harness Profile '{profile}'"))
    }

    fn create_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("new:{thread_id}"));
        Ok(())
    }

    fn activate_thread(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("activate:{thread_id}"));
        Ok(())
    }

    fn fork_thread(&self, source_id: &ThreadId, target_id: &ThreadId) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("fork:{source_id}:{target_id}"));
        Ok(())
    }

    fn submit_turn(
        &self,
        thread_id: &ThreadId,
        text: String,
        images: Vec<String>,
        execution_mode: ncx_protocol::ExecutionMode,
    ) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!(
            "submit:{thread_id}:{text}:{}:{execution_mode:?}",
            images.len()
        ));
        Ok(())
    }

    fn interrupt_latest(&self, thread_id: &ThreadId) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("interrupt:{thread_id}"));
        Ok(())
    }

    fn continue_goal(&self, thread_id: &ThreadId) -> Result<(), String> {
        if self.fail_goal_continue {
            return Err("worker unavailable".into());
        }
        self.calls
            .lock()
            .unwrap()
            .push(format!("goal-continue:{thread_id}"));
        Ok(())
    }

    fn runtime_status(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"model":"test-model"}))
    }

    fn refresh_ready(&self) -> Result<(), String> {
        self.calls.lock().unwrap().push("ready".into());
        Ok(())
    }

    fn set_workspace(&self, path: String) -> Result<String, String> {
        self.calls.lock().unwrap().push(format!("workspace:{path}"));
        Ok(path)
    }

    fn approve(
        &self,
        thread_id: Option<&ThreadId>,
        id: u64,
        decision: String,
    ) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!(
            "approve:{}:{id}:{decision}",
            thread_id.map(ToString::to_string).unwrap_or_default()
        ));
        Ok(())
    }

    fn answer(
        &self,
        thread_id: Option<&ThreadId>,
        id: u64,
        answer: Option<String>,
    ) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!(
            "answer:{}:{id}:{}",
            thread_id.map(ToString::to_string).unwrap_or_default(),
            answer.unwrap_or_default()
        ));
        Ok(())
    }

    fn read_settings(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"model":"test-model"}))
    }

    fn update_settings(
        &self,
        updates: std::collections::BTreeMap<String, String>,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("settings:{}", updates.len()));
        Ok(())
    }

    fn set_model(&self, model: String) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("model:{model}"));
        Ok(())
    }

    fn set_permission_mode(&self, mode: String) -> Result<(), String> {
        self.calls.lock().unwrap().push(format!("mode:{mode}"));
        Ok(())
    }

    fn read_model_catalog(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"providers":[]}))
    }

    fn apply_model_preset(
        &self,
        provider_id: String,
        model_id: String,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("preset:{provider_id}:{model_id}"));
        Ok(serde_json::json!({"model_id":model_id}))
    }

    fn list_custom_providers(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"id":"relay"}]))
    }

    fn save_custom_provider(
        &self,
        id: Option<String>,
        name: String,
        protocol: String,
        base_url: String,
        _api_key: Option<String>,
        models: Vec<String>,
    ) -> Result<serde_json::Value, String> {
        self.calls.lock().unwrap().push(format!(
            "provider-save:{}:{name}:{protocol}:{base_url}:{}",
            id.unwrap_or_default(),
            models.len()
        ));
        Ok(serde_json::json!({"id":"relay"}))
    }

    fn delete_custom_provider(&self, id: String) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("provider-delete:{id}"));
        Ok(())
    }

    fn discover_custom_provider_models(&self, id: String) -> Result<Vec<String>, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("provider-discover:{id}"));
        Ok(vec!["gpt-5.6-sol".into()])
    }

    fn activate_custom_provider(&self, id: String, model: String) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("provider-activate:{id}:{model}"));
        Ok(())
    }

    fn probe_custom_provider_chat(
        &self,
        id: String,
        model: String,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("provider-chat-probe:{id}:{model}"));
        Ok(serde_json::json!({"requested_model":model,"confirmed_model":model,"protocol":"openai"}))
    }

    fn harness_diagnostics(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({"llm":true}))
    }

    fn list_external_plugins(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"id":"demo.echo"}]))
    }

    fn install_external_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("external-install:{source}:{upgrade}"));
        Ok(serde_json::json!({"id":"demo.echo"}))
    }

    fn set_external_plugin_enabled(&self, id: String, enabled: bool) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("external-enabled:{id}:{enabled}"));
        Ok(())
    }

    fn list_memory(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"note":"remember"}]))
    }

    fn add_memory(&self, note: String, tags: Vec<String>) -> Result<bool, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("memory-add:{note}:{}", tags.len()));
        Ok(true)
    }

    fn consolidate_memory(&self) -> Result<u64, String> {
        self.calls.lock().unwrap().push("memory-consolidate".into());
        Ok(2)
    }

    fn list_codex_plugins(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"name":"demo"}]))
    }

    fn install_codex_plugin(
        &self,
        source: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("plugin-install:{source}:{upgrade}"));
        Ok(serde_json::json!({"name":"demo"}))
    }

    fn set_codex_plugin_enabled(&self, name: String, enabled: bool) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("plugin-enabled:{name}:{enabled}"));
        Ok(())
    }

    fn uninstall_codex_plugin(&self, name: String) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("plugin-uninstall:{name}"));
        Ok(())
    }

    fn list_marketplaces(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([{"name":"local"}]))
    }

    fn install_marketplace_plugin(
        &self,
        marketplace_path: String,
        plugin_name: String,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        self.calls.lock().unwrap().push(format!(
            "marketplace-install:{marketplace_path}:{plugin_name}:{upgrade}"
        ));
        Ok(serde_json::json!({"name":plugin_name}))
    }

    fn search_dsh_marketplace(
        &self,
        source: String,
        _manifest_url: Option<String>,
        query: String,
    ) -> Result<serde_json::Value, String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("dsh-search:{source}:{query}"));
        Ok(serde_json::json!({"items":[]}))
    }

    fn preview_dsh_marketplace_plugin(
        &self,
        item: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.calls.lock().unwrap().push(format!(
            "dsh-preview:{}",
            item["id"].as_str().unwrap_or_default()
        ));
        Ok(serde_json::json!({"compatibility":"convertible"}))
    }

    fn install_dsh_marketplace_plugin(
        &self,
        item: serde_json::Value,
        upgrade: bool,
    ) -> Result<serde_json::Value, String> {
        self.calls.lock().unwrap().push(format!(
            "dsh-install:{}:{upgrade}",
            item["id"].as_str().unwrap_or_default()
        ));
        Ok(serde_json::json!({"name":item["id"]}))
    }
}
