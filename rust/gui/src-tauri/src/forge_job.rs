use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

use crate::forge_runtime::ForgeRuntimePaths;

#[derive(Clone, Debug)]
pub struct ForgeJobInput {
    pub rounds: u8,
    pub repeats: u8,
    pub timeout_s: u64,
    pub budget_s: u64,
    pub teacher: String,
    pub accept_margin: u8,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeJobSummary {
    mode: String,
    rounds: usize,
    accepted_rounds: usize,
    champion_train: Option<u64>,
    champion_holdout: Option<u64>,
    test_baseline: Option<u64>,
    test_champion: Option<u64>,
    test_runs: Option<u64>,
    report_file: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeJobStatus {
    pub generation: u64,
    pub status: String,
    pub started_at_ms: Option<i64>,
    pub rounds: Option<u8>,
    pub repeats: Option<u8>,
    pub timeout_s: Option<u64>,
    pub budget_s: Option<u64>,
    pub teacher: Option<String>,
    pub accept_margin: Option<u8>,
    pub summary: Option<ForgeJobSummary>,
    pub error: Option<String>,
}

impl ForgeJobStatus {
    fn idle() -> Self {
        Self {
            generation: 0,
            status: "idle".into(),
            started_at_ms: None,
            rounds: None,
            repeats: None,
            timeout_s: None,
            budget_s: None,
            teacher: None,
            accept_margin: None,
            summary: None,
            error: None,
        }
    }
}

struct ForgeJobState {
    status: ForgeJobStatus,
    /// Workspace that owns the current status projection. The owner remains
    /// attached after completion so another workspace cannot read old job
    /// details through this process-global coordinator.
    owner_workspace: Option<PathBuf>,
}

pub struct ForgeJobCoordinator {
    state: Mutex<ForgeJobState>,
    cancelled: Arc<AtomicBool>,
    pid: AtomicU32,
    #[cfg(windows)]
    owner: Mutex<Option<WindowsJob>>,
}

impl Default for ForgeJobCoordinator {
    fn default() -> Self {
        Self {
            state: Mutex::new(ForgeJobState {
                status: ForgeJobStatus::idle(),
                owner_workspace: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
            pid: AtomicU32::new(0),
            #[cfg(windows)]
            owner: Mutex::new(None),
        }
    }
}

impl ForgeJobCoordinator {
    pub fn start(
        self: &Arc<Self>,
        input: ForgeJobInput,
        runtime: ForgeRuntimePaths,
        workspace: PathBuf,
    ) -> Result<ForgeJobStatus, String> {
        validate_input(&input)?;
        if !workspace.is_dir() {
            return Err("当前工作区不存在".into());
        }
        let output_root = workspace.join(".ncx").join("forge");
        let runs = output_root.join("runs");
        let genomes = output_root.join("genomes");
        std::fs::create_dir_all(&runs).map_err(|_| "无法创建 Forge 报告目录".to_string())?;
        std::fs::create_dir_all(&genomes).map_err(|_| "无法创建 Forge 基因组目录".to_string())?;

        let generation = self.begin(&input, &workspace)?;
        let mut command = forge_command(&runtime, &input, &workspace, &runs, &genomes);
        let mut child = command.spawn().map_err(|_| {
            self.finish(
                generation,
                "failed",
                None,
                Some("无法启动 Forge 运行时".into()),
            );
            "无法启动 Forge 运行时".to_string()
        })?;
        if let Err(error) = self.own_process(child.id()) {
            terminate_tree(child.id());
            let _ = child.wait();
            self.finish(generation, "failed", None, Some(error.clone()));
            return Err(error);
        }
        self.pid.store(child.id(), Ordering::SeqCst);
        let coordinator = self.clone();
        std::thread::Builder::new()
            .name(format!("ncx-forge-job-{generation}"))
            .spawn(move || supervise(coordinator, generation, input, child, runs))
            .map_err(|_| {
                terminate_tree(self.pid.swap(0, Ordering::SeqCst));
                self.finish(
                    generation,
                    "failed",
                    None,
                    Some("无法监控 Forge 任务".into()),
                );
                "无法监控 Forge 任务".to_string()
            })?;
        self.status()
    }

    pub fn status(&self) -> Result<ForgeJobStatus, String> {
        self.state
            .lock()
            .map(|state| state.status.clone())
            .map_err(|_| "Forge 状态不可用".to_string())
    }

    /// Return only the status projection owned by `workspace`. A different
    /// workspace, or a poller carrying an obsolete generation, receives a
    /// neutral idle snapshot and cannot learn another project's job details.
    pub fn status_for_workspace(
        &self,
        workspace: &Path,
        expected_generation: Option<u64>,
    ) -> Result<ForgeJobStatus, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "Forge 状态不可用".to_string())?;
        if !owns_workspace(&state, workspace)
            || expected_generation.is_some_and(|generation| generation != state.status.generation)
        {
            return Ok(ForgeJobStatus::idle());
        }
        Ok(state.status.clone())
    }

    /// Cancel only the exact generation owned by this workspace. Delayed
    /// requests from an earlier job therefore cannot cancel a replacement,
    /// even if the user has switched away and back to the same directory.
    pub fn cancel_for_workspace(
        &self,
        workspace: &Path,
        expected_generation: u64,
    ) -> Result<ForgeJobStatus, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Forge 状态不可用".to_string())?;
        if !owns_workspace(&state, workspace) {
            return Err("当前项目没有可取消的 Forge 任务".to_string());
        }
        if state.status.generation != expected_generation {
            return Err("Forge 任务已被更新，拒绝取消旧任务".to_string());
        }
        if state.status.status == "running" {
            self.cancelled.store(true, Ordering::SeqCst);
            state.status.status = "cancelling".into();
        }
        Ok(state.status.clone())
    }

    fn begin(&self, input: &ForgeJobInput, workspace: &Path) -> Result<u64, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Forge 状态不可用".to_string())?;
        if matches!(state.status.status.as_str(), "running" | "cancelling") {
            return Err("Forge 训练任务正在运行".into());
        }
        state.status.generation = state.status.generation.saturating_add(1);
        state.status.status = "running".into();
        state.status.started_at_ms = Some(epoch_millis());
        state.status.rounds = Some(input.rounds);
        state.status.repeats = Some(input.repeats);
        state.status.timeout_s = Some(input.timeout_s);
        state.status.budget_s = Some(input.budget_s);
        state.status.teacher = Some(input.teacher.clone());
        state.status.accept_margin = Some(input.accept_margin);
        state.status.summary = None;
        state.status.error = None;
        state.owner_workspace = Some(workspace.to_path_buf());
        self.cancelled.store(false, Ordering::SeqCst);
        Ok(state.status.generation)
    }

    fn finish(
        &self,
        generation: u64,
        status: &str,
        summary: Option<ForgeJobSummary>,
        error: Option<String>,
    ) {
        self.pid.store(0, Ordering::SeqCst);
        #[cfg(windows)]
        if let Ok(mut owner) = self.owner.lock() {
            owner.take();
        }
        if let Ok(mut state) = self.state.lock() {
            if state.status.generation == generation {
                state.status.status = status.into();
                state.status.summary = summary;
                state.status.error = error;
            }
        }
    }

    #[cfg(windows)]
    fn own_process(&self, pid: u32) -> Result<(), String> {
        let job = WindowsJob::new()?;
        job.assign(pid)?;
        self.owner
            .lock()
            .map_err(|_| "Forge 进程所有权不可用".to_string())?
            .replace(job);
        Ok(())
    }

    #[cfg(not(windows))]
    fn own_process(&self, _pid: u32) -> Result<(), String> {
        // Unix-like targets isolate the child in its own process group in
        // `configure_process_group`; `terminate_tree` can therefore stop the
        // complete Forge process tree without a Windows Job Object.
        Ok(())
    }

    fn terminate_owned(&self, fallback_pid: u32) {
        #[cfg(windows)]
        if let Ok(mut owner) = self.owner.lock() {
            if let Some(job) = owner.take() {
                job.terminate();
                return;
            }
        }
        terminate_tree(fallback_pid);
    }
}

fn owns_workspace(state: &ForgeJobState, workspace: &Path) -> bool {
    let Some(owner) = &state.owner_workspace else {
        return false;
    };
    let Ok(workspace) = std::fs::canonicalize(workspace) else {
        return false;
    };
    let Ok(owner) = std::fs::canonicalize(owner) else {
        return false;
    };
    #[cfg(windows)]
    {
        owner
            .to_string_lossy()
            .eq_ignore_ascii_case(&workspace.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        owner == workspace
    }
}

impl Drop for ForgeJobCoordinator {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.terminate_owned(self.pid.swap(0, Ordering::SeqCst));
    }
}

fn validate_input(input: &ForgeJobInput) -> Result<(), String> {
    for (valid, message) in [
        ((1..=5).contains(&input.rounds), "训练轮数必须是 1–5"),
        ((1..=3).contains(&input.repeats), "重复评测必须是 1–3"),
        (
            (30..=300).contains(&input.timeout_s),
            "单任务超时必须是 30–300 秒",
        ),
        (
            (60..=3600).contains(&input.budget_s),
            "总时限必须是 60–3600 秒",
        ),
        (
            (1..=3).contains(&input.accept_margin),
            "接受门差值必须是 1–3",
        ),
        (
            matches!(input.teacher.as_str(), "panel" | "codex" | "claude" | "api"),
            "教师必须是 panel、codex、claude 或 api",
        ),
    ] {
        if !valid {
            return Err(message.into());
        }
    }
    Ok(())
}

fn forge_command(
    runtime: &ForgeRuntimePaths,
    input: &ForgeJobInput,
    workspace: &Path,
    runs: &Path,
    genomes: &Path,
) -> Command {
    let mut command = Command::new(&runtime.python);
    command
        .arg(&runtime.script)
        .arg("--train")
        .arg("--rounds")
        .arg(input.rounds.to_string())
        .arg("--repeats")
        .arg(input.repeats.to_string())
        .arg("--timeout")
        .arg(input.timeout_s.to_string())
        .arg("--budget-s")
        .arg(input.budget_s.to_string())
        .arg("--teacher")
        .arg(&input.teacher)
        .arg("--accept-margin")
        .arg(input.accept_margin.to_string())
        .current_dir(workspace)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("NCX_FORGE_NCX_BIN", &runtime.agent)
        .env("NCX_FORGE_RUNS_DIR", runs)
        .env("NCX_FORGE_GENOMES_DIR", genomes)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_process_group(&mut command);
    command
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0000_0200 | 0x0800_0000);
}

#[cfg(not(windows))]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

fn supervise(
    coordinator: Arc<ForgeJobCoordinator>,
    generation: u64,
    input: ForgeJobInput,
    mut child: Child,
    runs: PathBuf,
) {
    let started = Instant::now();
    let started_system = SystemTime::now();
    loop {
        if coordinator.cancelled.load(Ordering::SeqCst) {
            coordinator.terminate_owned(child.id());
            let _ = child.wait();
            coordinator.finish(generation, "cancelled", None, None);
            return;
        }
        if started.elapsed() >= Duration::from_secs(input.budget_s) {
            coordinator.terminate_owned(child.id());
            let _ = child.wait();
            coordinator.finish(
                generation,
                "budgetExceeded",
                None,
                Some("已达到总时限，完整进程树已停止".into()),
            );
            return;
        }
        match child.try_wait() {
            Ok(Some(exit)) if exit.success() => {
                match newest_summary(&runs, started_system) {
                    Ok(summary) => coordinator.finish(generation, "completed", Some(summary), None),
                    Err(error) => coordinator.finish(generation, "failed", None, Some(error)),
                }
                return;
            }
            Ok(Some(_)) => {
                coordinator.finish(
                    generation,
                    "failed",
                    None,
                    Some("Forge 任务失败，未生成可验证结果".into()),
                );
                return;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => {
                coordinator.terminate_owned(child.id());
                coordinator.finish(
                    generation,
                    "failed",
                    None,
                    Some("无法读取 Forge 进程状态".into()),
                );
                return;
            }
        }
    }
}

fn newest_summary(runs: &Path, started: SystemTime) -> Result<ForgeJobSummary, String> {
    let mut candidates = std::fs::read_dir(runs)
        .map_err(|_| "Forge 报告目录不可读".to_string())?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                && entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .is_ok_and(|time| time >= started)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok());
    let report = candidates
        .pop()
        .ok_or_else(|| "Forge 未生成可验证 lineage".to_string())?;
    let bytes = std::fs::read(report.path()).map_err(|_| "Forge lineage 不可读".to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Forge lineage 超过安全大小限制".into());
    }
    safe_summary(
        &serde_json::from_slice(&bytes).map_err(|_| "Forge lineage 格式无效".to_string())?,
        &report.file_name().to_string_lossy(),
    )
}

fn safe_summary(value: &serde_json::Value, report_file: &str) -> Result<ForgeJobSummary, String> {
    let rounds = value
        .get("rounds")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "Forge lineage 缺少轮次证据".to_string())?;
    let accepted_rounds = rounds
        .iter()
        .filter(|round| {
            round
                .get("accept")
                .and_then(|accept| accept.get("teacher"))
                .and_then(serde_json::Value::as_str)
                .is_some()
        })
        .count();
    let null = serde_json::Value::Null;
    let champion = value.get("champion").unwrap_or(&null);
    let test = value.get("test").unwrap_or(&null);
    Ok(ForgeJobSummary {
        mode: value
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("train")
            .to_string(),
        rounds: rounds.len(),
        accepted_rounds,
        champion_train: champion.get("train").and_then(serde_json::Value::as_u64),
        champion_holdout: champion.get("holdout").and_then(serde_json::Value::as_u64),
        test_baseline: test.get("baseline").and_then(serde_json::Value::as_u64),
        test_champion: test.get("champion").and_then(serde_json::Value::as_u64),
        test_runs: test.get("runs").and_then(serde_json::Value::as_u64),
        report_file: report_file.to_string(),
    })
}

#[cfg(windows)]
struct WindowsJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    fn new() -> Result<Self, String> {
        use windows_sys::Win32::System::JobObjects::{
            JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        let handle = unsafe {
            windows_sys::Win32::System::JobObjects::CreateJobObjectW(
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if handle.is_null() {
            return Err("无法创建 Forge Windows Job Object".into());
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const std::ffi::c_void,
                std::mem::size_of_val(&limits) as u32,
            )
        };
        if configured == 0 {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            return Err("无法配置 Forge Windows Job Object".into());
        }
        Ok(Self { handle })
    }

    fn assign(&self, pid: u32) -> Result<(), String> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };
        let process = unsafe {
            OpenProcess(
                PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            )
        };
        if process.is_null() {
            return Err("无法取得 Forge 进程所有权".into());
        }
        let assigned = unsafe {
            windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(self.handle, process)
        };
        unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
        if assigned == 0 {
            return Err("无法将 Forge 进程加入 Windows Job Object".into());
        }
        Ok(())
    }

    fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle) };
    }
}

fn terminate_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/pid", &pid.to_string(), "/t", "/f"])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{pid}")])
            .status();
    }
}

fn epoch_millis() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workspace(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ncx-forge-owner-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn input() -> ForgeJobInput {
        ForgeJobInput {
            rounds: 2,
            repeats: 1,
            timeout_s: 120,
            budget_s: 600,
            teacher: "panel".into(),
            accept_margin: 1,
        }
    }

    #[test]
    fn rejects_unbounded_or_unsafe_parameters() {
        assert!(validate_input(&input()).is_ok());
        for mutate in [
            |value: &mut ForgeJobInput| value.rounds = 0,
            |value: &mut ForgeJobInput| value.repeats = 4,
            |value: &mut ForgeJobInput| value.timeout_s = 301,
            |value: &mut ForgeJobInput| value.budget_s = 59,
            |value: &mut ForgeJobInput| value.teacher = "shell".into(),
            |value: &mut ForgeJobInput| value.accept_margin = 0,
        ] {
            let mut value = input();
            mutate(&mut value);
            assert!(validate_input(&value).is_err());
        }
    }

    #[test]
    fn summary_whitelists_metrics_and_ignores_sensitive_fields() {
        let value = serde_json::json!({
            "rounds": [{"accept":{"teacher":"codex"},"secret":"TOKEN"}],
            "champion": {"train": 3, "holdout": 2, "diff_vs_baseline": "SECRET"},
            "test": {"baseline": 1, "champion": 2, "runs": 3},
            "rawTrajectory": "SECRET"
        });
        let encoded =
            serde_json::to_string(&safe_summary(&value, "lineage.json").unwrap()).unwrap();
        assert!(encoded.contains("acceptedRounds"));
        assert!(!encoded.contains("SECRET"));
        assert!(!encoded.contains("TOKEN"));
    }

    fn running_job(
        coordinator: &ForgeJobCoordinator,
        generation: u64,
        workspace: PathBuf,
    ) -> Arc<AtomicBool> {
        let cancelled = coordinator.cancelled.clone();
        let mut state = coordinator.state.lock().unwrap();
        state.status.generation = generation;
        state.status.status = "running".into();
        state.owner_workspace = Some(workspace);
        cancelled.store(false, Ordering::SeqCst);
        cancelled
    }

    #[test]
    fn workspace_and_generation_fence_status_and_cancel() {
        let coordinator = ForgeJobCoordinator::default();
        let owner = test_workspace("owner");
        let other = test_workspace("other");
        let cancelled = running_job(&coordinator, 17, owner.clone());

        let hidden = coordinator.status_for_workspace(&other, None).unwrap();
        assert_eq!(hidden.status, "idle");
        assert_eq!(hidden.generation, 0);
        assert!(coordinator.cancel_for_workspace(&other, 17).is_err());
        assert!(!cancelled.load(Ordering::SeqCst));

        let stale = coordinator.status_for_workspace(&owner, Some(16)).unwrap();
        assert_eq!(stale.status, "idle");
        assert!(coordinator.cancel_for_workspace(&owner, 16).is_err());
        assert!(!cancelled.load(Ordering::SeqCst));

        let status = coordinator.cancel_for_workspace(&owner, 17).unwrap();
        assert_eq!(status.status, "cancelling");
        assert!(cancelled.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(owner);
        let _ = std::fs::remove_dir_all(other);
    }

    #[cfg(windows)]
    #[test]
    fn terminate_tree_stops_owned_descendant_before_delayed_write() {
        use std::os::windows::process::CommandExt;

        let root = std::env::temp_dir().join(format!(
            "ncx-forge-tree-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("descendant-survived.txt");
        let child_script = root.join("child.ps1");
        let parent_script = root.join("parent.ps1");
        let escaped_marker = marker.display().to_string().replace(char::from(39), "''");
        let escaped_child = child_script
            .display()
            .to_string()
            .replace(char::from(39), "''");
        std::fs::write(
            &child_script,
            format!("Start-Sleep -Seconds 2\nSet-Content -LiteralPath '{escaped_marker}' -Value survived\n"),
        )
        .unwrap();
        std::fs::write(
            &parent_script,
            format!(
                "Start-Sleep -Milliseconds 800\nStart-Process powershell.exe -ArgumentList @('-NoProfile','-File','{escaped_child}') -WindowStyle Hidden\nStart-Sleep -Seconds 30\n"
            ),
        )
        .unwrap();
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-File"])
            .arg(&parent_script)
            .creation_flags(0x0000_0200 | 0x0800_0000)
            .spawn()
            .unwrap();
        let job = WindowsJob::new().unwrap();
        job.assign(child.id()).unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        job.terminate();
        drop(job);
        let _ = child.wait();
        std::thread::sleep(Duration::from_secs(3));
        assert!(!marker.exists(), "Forge descendant escaped cancellation");
        let _ = std::fs::remove_dir_all(root);
    }
}
