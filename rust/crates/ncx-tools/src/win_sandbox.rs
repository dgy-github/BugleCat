//! Windows OS-level write isolation for sandboxed commands.
//!
//! Confined commands run under a privilege-stripped, **Low integrity** token.
//! Mandatory Integrity Control then denies writes to every object labeled
//! Medium or higher, which is the implicit label of the user's files. The
//! writable roots (the workspace plus configured writable roots) and a
//! per-run temp directory carry an inheritable Low mandatory label
//! (`NO_WRITE_UP`, object + container inherit), so writes there succeed.
//!
//! Reads are not restricted, and neither is the network: a Low IL process
//! can still open sockets. Labels are persistent NTFS metadata; they are
//! applied once per root (checked before writing, cached per process).
//!
//! Every failure here is reported as a `String` note so the caller can fall
//! back to an unconfined run and tell the model and user why.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{c_void, OsStr};
use std::future::Future;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, SetHandleInformation, GENERIC_READ, GENERIC_WRITE, HANDLE,
    HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSidToSidW, GetNamedSecurityInfoW, SetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    AddMandatoryAce, CreateRestrictedToken, EqualSid, GetAce, GetLengthSid, InitializeAcl,
    SetTokenInformation, TokenIntegrityLevel, ACE_HEADER, ACL, ACL_REVISION, CONTAINER_INHERIT_ACE,
    DISABLE_MAX_PRIVILEGE, INHERIT_ONLY_ACE, LABEL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE,
    PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES, SYSTEM_MANDATORY_LABEL_ACE,
    TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, GetVolumeInformationW, GetVolumePathNameW, FILE_READ_ATTRIBUTES,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, OPEN_EXISTING,
};
use windows_sys::Win32::System::SystemServices::{
    FILE_PERSISTENT_ACLS, SE_GROUP_INTEGRITY, SYSTEM_MANDATORY_LABEL_ACE_TYPE,
    SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, DeleteProcThreadAttributeList, GetCurrentProcess, GetExitCodeProcess,
    InitializeProcThreadAttributeList, OpenProcessToken, ResumeThread, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

use crate::executor::win_job::{Job, JobLimits};

const LOW_INTEGRITY_SID: &str = "S-1-16-4096";
const MEDIUM_INTEGRITY_SID: &str = "S-1-16-8192";
const DRIVE_REMOTE: u32 = 4;
const INHERIT_FLAGS: u32 = OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE;

/// Workspace entries that stay write-protected from confined commands even
/// though they sit inside a writable root (mirrors `is_protected_write_path`).
const PROTECTED_CHILDREN: [&str; 2] = [".git\\hooks", ".git\\config"];

/// Roots already verified to carry the Low label in this process.
static LABELED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
static PIPE_COUNTER: AtomicU64 = AtomicU64::new(0);
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn last_error(context: &str) -> String {
    format!("{context}: {}", io::Error::last_os_error())
}

/// A SID allocated by `ConvertStringSidToSidW`.
struct Sid(PSID);

impl Sid {
    fn parse(text: &str) -> Result<Sid, String> {
        let mut sid: PSID = null_mut();
        let text = wide(text);
        if unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut sid) } == 0 {
            return Err(last_error("ConvertStringSidToSid failed"));
        }
        Ok(Sid(sid))
    }
}

impl Drop for Sid {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0 as _);
        }
    }
}

/// Per-run writable temp directory, removed when the run ends.
pub(crate) struct RunTempDir(PathBuf);

impl RunTempDir {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RunTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Make every root writable for Low IL processes and create the per-run temp
/// directory. `Err` carries the reason confinement is not possible.
pub(crate) fn prepare(roots: &[PathBuf]) -> Result<RunTempDir, String> {
    for root in roots {
        let root = std::fs::canonicalize(root)
            .map_err(|e| format!("cannot resolve writable root {}: {e}", root.display()))?;
        ensure_low_label(&root)?;
        protect_children(&root)?;
    }
    let base = std::env::temp_dir().join("ncx-sandbox");
    std::fs::create_dir_all(&base)
        .map_err(|e| format!("cannot create sandbox temp dir {}: {e}", base.display()))?;
    let base = std::fs::canonicalize(&base)
        .map_err(|e| format!("cannot resolve sandbox temp dir {}: {e}", base.display()))?;
    ensure_low_label(&base)?;
    let run = base.join(format!(
        "run-{}-{}",
        std::process::id(),
        RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    // Created by this (Medium) process; it inherits the Low label from `base`.
    std::fs::create_dir_all(&run)
        .map_err(|e| format!("cannot create sandbox temp dir {}: {e}", run.display()))?;
    Ok(RunTempDir(strip_verbatim(&run)))
}

fn strip_verbatim(path: &Path) -> PathBuf {
    ncx_sandbox::strip_verbatim_prefix(path.to_path_buf())
}

/// Refuse roots whose labeling would open up far more than a project tree.
fn check_root_is_safe(root: &Path) -> Result<(), String> {
    let plain = strip_verbatim(root);
    if plain.parent().is_none() {
        return Err(format!(
            "refusing to label drive root {} as writable",
            plain.display()
        ));
    }
    let lower = |p: &Path| p.to_string_lossy().to_ascii_lowercase();
    let root_l = lower(&plain);
    let is_ancestor_or_same = |other: &str| {
        let other = other.trim_end_matches('\\').to_ascii_lowercase();
        !other.is_empty() && (other == root_l || other.starts_with(&format!("{root_l}\\")))
    };
    let is_inside = |other: &str| {
        let other = other.trim_end_matches('\\').to_ascii_lowercase();
        !other.is_empty() && (root_l == other || root_l.starts_with(&format!("{other}\\")))
    };
    if let Ok(profile) = std::env::var("USERPROFILE") {
        if is_ancestor_or_same(&profile) {
            return Err(format!(
                "refusing to label {} as writable: it contains the user profile",
                plain.display()
            ));
        }
    }
    for var in [
        "SYSTEMROOT",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
    ] {
        if let Ok(dir) = std::env::var(var) {
            if is_inside(&dir) || is_ancestor_or_same(&dir) {
                return Err(format!(
                    "refusing to label {} as writable: it overlaps {dir}",
                    plain.display()
                ));
            }
        }
    }
    Ok(())
}

/// Reject volumes where an integrity label is either impossible (no ACL
/// support, e.g. FAT/exFAT) or not enforced (network shares are checked by the
/// server against the network logon, not the local Low IL token).
fn check_volume_supports_labels(root: &Path) -> Result<(), String> {
    let path_w = wide(root);
    let mut volume = vec![0u16; 1024];
    if unsafe { GetVolumePathNameW(path_w.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) } == 0
    {
        return Err(last_error(&format!(
            "cannot determine the volume of {}",
            root.display()
        )));
    }
    if unsafe { GetDriveTypeW(volume.as_ptr()) } == DRIVE_REMOTE {
        return Err(format!(
            "{} is on a network drive; integrity labels are not enforced there",
            strip_verbatim(root).display()
        ));
    }
    let mut flags = 0u32;
    let ok = unsafe {
        GetVolumeInformationW(
            volume.as_ptr(),
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut flags,
            null_mut(),
            0,
        )
    };
    if ok == 0 {
        return Err(last_error(&format!(
            "cannot query the file system of {}",
            root.display()
        )));
    }
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Err(format!(
            "the file system of {} has no ACL support (e.g. FAT/exFAT)",
            strip_verbatim(root).display()
        ));
    }
    Ok(())
}

/// Apply the inheritable Low `NO_WRITE_UP` label to `root` once.
fn ensure_low_label(root: &Path) -> Result<(), String> {
    let mut guard = LABELED.lock().unwrap_or_else(|e| e.into_inner());
    let labeled = guard.get_or_insert_with(HashSet::new);
    if labeled.contains(root) {
        return Ok(());
    }
    check_root_is_safe(root)?;
    check_volume_supports_labels(root)?;
    let low = Sid::parse(LOW_INTEGRITY_SID)?;
    if !has_label(root, &low, INHERIT_FLAGS)? {
        // Propagates the inherited label to the existing tree; this is the
        // one slow step for a large workspace and only happens once.
        set_label(root, &low, INHERIT_FLAGS)?;
        if !has_label(root, &low, INHERIT_FLAGS)? {
            return Err(format!(
                "the integrity label on {} did not persist",
                strip_verbatim(root).display()
            ));
        }
    }
    labeled.insert(root.to_path_buf());
    Ok(())
}

/// Pin an explicit Medium label on protected entries. Explicit ACEs precede
/// inherited ones and only the first label ACE counts, so the workspace's
/// inherited Low label no longer makes them writable in place. This is
/// defense in depth only: `NO_WRITE_UP` does not cover `DELETE`, so a Low
/// process can still replace them by renaming over them inside the Low-labeled
/// `.git`. (A protected SACL would need `SeSecurityPrivilege`.)
fn protect_children(root: &Path) -> Result<(), String> {
    let medium = Sid::parse(MEDIUM_INTEGRITY_SID)?;
    for child in PROTECTED_CHILDREN {
        let path = root.join(child);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let flags = if meta.is_dir() { INHERIT_FLAGS } else { 0 };
        if !has_label(&path, &medium, flags)? {
            set_label(&path, &medium, flags)?;
        }
    }
    Ok(())
}

/// True when the first mandatory label ACE on `path` is `sid` with
/// `NO_WRITE_UP` and exactly the requested inheritance flags.
fn has_label(path: &Path, sid: &Sid, inherit: u32) -> Result<bool, String> {
    let path_w = wide(path);
    let mut sacl: *mut ACL = null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    let err = unsafe {
        GetNamedSecurityInfoW(
            path_w.as_ptr(),
            SE_FILE_OBJECT,
            LABEL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut sacl,
            &mut sd,
        )
    };
    if err != 0 {
        return Err(format!(
            "cannot read the integrity label of {}: {}",
            strip_verbatim(path).display(),
            io::Error::from_raw_os_error(err as i32)
        ));
    }
    let mut found = false;
    unsafe {
        if !sacl.is_null() {
            for index in 0..u32::from((*sacl).AceCount) {
                let mut ace: *mut c_void = null_mut();
                if GetAce(sacl, index, &mut ace) == 0 {
                    continue;
                }
                let header = &*(ace as *const ACE_HEADER);
                if u32::from(header.AceType) != SYSTEM_MANDATORY_LABEL_ACE_TYPE {
                    continue;
                }
                let label = &*(ace as *const SYSTEM_MANDATORY_LABEL_ACE);
                let ace_sid = std::ptr::addr_of!(label.SidStart) as PSID;
                let flags = u32::from(header.AceFlags);
                found = EqualSid(ace_sid, sid.0) != 0
                    && label.Mask & SYSTEM_MANDATORY_LABEL_NO_WRITE_UP != 0
                    && flags & (INHERIT_FLAGS | INHERIT_ONLY_ACE) == inherit;
                // Only the first label ACE is honored.
                break;
            }
        }
        LocalFree(sd as _);
    }
    Ok(found)
}

fn set_label(path: &Path, sid: &Sid, inherit: u32) -> Result<(), String> {
    let sid_len = unsafe { GetLengthSid(sid.0) } as usize;
    let acl_len =
        std::mem::size_of::<ACL>() + std::mem::size_of::<SYSTEM_MANDATORY_LABEL_ACE>() + sid_len;
    // u32 storage keeps the ACL DWORD-aligned.
    let mut buf = vec![0u32; acl_len.div_ceil(4)];
    let acl = buf.as_mut_ptr() as *mut ACL;
    let path_w = wide(path);
    let err = unsafe {
        if InitializeAcl(acl, (buf.len() * 4) as u32, ACL_REVISION) == 0
            || AddMandatoryAce(
                acl,
                ACL_REVISION,
                inherit,
                SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
                sid.0,
            ) == 0
        {
            return Err(last_error("cannot build the integrity label ACL"));
        }
        SetNamedSecurityInfoW(
            path_w.as_ptr(),
            SE_FILE_OBJECT,
            LABEL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null(),
            acl,
        )
    };
    if err != 0 {
        return Err(format!(
            "cannot set the integrity label of {}: {}",
            strip_verbatim(path).display(),
            io::Error::from_raw_os_error(err as i32)
        ));
    }
    Ok(())
}

/// A privilege-stripped primary token at Low integrity.
fn low_integrity_token() -> Result<OwnedHandle, String> {
    unsafe {
        let mut own: HANDLE = null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
            &mut own,
        ) == 0
        {
            return Err(last_error("OpenProcessToken failed"));
        }
        let own = OwnedHandle::from_raw_handle(own);
        let mut restricted: HANDLE = null_mut();
        if CreateRestrictedToken(
            own.as_raw_handle(),
            DISABLE_MAX_PRIVILEGE,
            0,
            null(),
            0,
            null(),
            0,
            null(),
            &mut restricted,
        ) == 0
        {
            return Err(last_error("CreateRestrictedToken failed"));
        }
        let restricted = OwnedHandle::from_raw_handle(restricted);
        let low = Sid::parse(LOW_INTEGRITY_SID)?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: low.0,
                Attributes: SE_GROUP_INTEGRITY as u32,
            },
        };
        let size = std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(low.0);
        if SetTokenInformation(
            restricted.as_raw_handle(),
            TokenIntegrityLevel,
            &label as *const _ as *const c_void,
            size,
        ) == 0
        {
            return Err(last_error("setting the Low integrity level failed"));
        }
        Ok(restricted)
    }
}

/// Create a named pipe: the async server end for this process and a
/// synchronous client handle for the child. `inbound` = child writes.
fn pipe_pair(inbound: bool) -> io::Result<(NamedPipeServer, OwnedHandle)> {
    let name = format!(
        r"\\.\pipe\ncx-sandbox-{}-{}",
        std::process::id(),
        PIPE_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .access_inbound(inbound)
        .access_outbound(!inbound)
        .max_instances(1)
        .create(&name)?;
    let access = if inbound {
        GENERIC_WRITE | FILE_READ_ATTRIBUTES
    } else {
        GENERIC_READ | FILE_WRITE_ATTRIBUTES
    };
    let name_w = wide(&name);
    let client = unsafe {
        CreateFileW(
            name_w.as_ptr(),
            access,
            0,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if client == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let client = unsafe { OwnedHandle::from_raw_handle(client) };
    connect_now(&server)?;
    Ok((server, client))
}

/// Complete the server-side connect for an already-opened client. It finishes
/// on the first poll (`ERROR_PIPE_CONNECTED`); until it runs, mio schedules no
/// reads and the pipe would never yield data.
fn connect_now(server: &NamedPipeServer) -> io::Result<()> {
    let mut connect = std::pin::pin!(server.connect());
    let mut cx = Context::from_waker(Waker::noop());
    match connect.as_mut().poll(&mut cx) {
        Poll::Ready(result) => result,
        Poll::Pending => Err(io::Error::other("pipe client did not connect")),
    }
}

fn null_device() -> io::Result<OwnedHandle> {
    let name = wide("NUL");
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// `CREATE_UNICODE_ENVIRONMENT` block, sorted case-insensitively.
fn environment_block(env: &HashMap<String, String>) -> Vec<u16> {
    let sorted: BTreeMap<String, (&String, &String)> = env
        .iter()
        .map(|(key, value)| (key.to_uppercase(), (key, value)))
        .collect();
    let mut block = Vec::new();
    for (key, value) in sorted.values() {
        block.extend(OsStr::new(&format!("{key}={value}")).encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// A Low IL child process created by [`spawn_low_integrity`].
pub(crate) struct LowIlChild {
    process: OwnedHandle,
    _temp: RunTempDir,
}

impl LowIlChild {
    pub(crate) fn try_wait(&self) -> io::Result<Option<i32>> {
        let handle = self.process.as_raw_handle();
        if unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
            return Ok(None);
        }
        let mut code = 0u32;
        if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(code as i32))
    }

    /// Poll for exit with a short backoff; cancel-safe, no extra threads.
    pub(crate) async fn wait(&self) -> io::Result<i32> {
        let mut delay = Duration::from_millis(2);
        loop {
            if let Some(code) = self.try_wait()? {
                return Ok(code);
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_millis(50));
        }
    }

    pub(crate) fn kill(&self) {
        unsafe {
            TerminateProcess(self.process.as_raw_handle(), 1);
        }
    }
}

pub(crate) struct LowIlSpawn {
    pub child: LowIlChild,
    pub stdin: Option<NamedPipeServer>,
    pub stdout: NamedPipeServer,
    pub stderr: NamedPipeServer,
    pub job: Option<Job>,
}

/// Start `application` with `command_line` at Low integrity, suspended until
/// it has been placed in a fresh Job Object.
pub(crate) fn spawn_low_integrity(
    application: &str,
    command_line: &str,
    cwd: &Path,
    env: &HashMap<String, String>,
    pipe_stdin: bool,
    limits: &JobLimits,
    temp: RunTempDir,
) -> Result<LowIlSpawn, String> {
    let token = low_integrity_token()?;
    let pipe_err = |e: io::Error| format!("cannot create stdio pipes: {e}");
    let (stdout, stdout_child) = pipe_pair(true).map_err(pipe_err)?;
    let (stderr, stderr_child) = pipe_pair(true).map_err(pipe_err)?;
    let (stdin, stdin_child) = if pipe_stdin {
        let (server, client) = pipe_pair(false).map_err(pipe_err)?;
        (Some(server), client)
    } else {
        (None, null_device().map_err(pipe_err)?)
    };
    let handles: [HANDLE; 3] = [
        stdin_child.as_raw_handle(),
        stdout_child.as_raw_handle(),
        stderr_child.as_raw_handle(),
    ];

    let application_w = wide(application);
    let mut command_line_w = wide(command_line);
    let cwd_w = wide(cwd);
    let env_block = environment_block(env);

    unsafe {
        // The handle list restricts inheritance to exactly these handles;
        // they only need to be inheritable for the CreateProcess call.
        for handle in handles {
            if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) == 0 {
                return Err(last_error("cannot mark stdio handles inheritable"));
            }
        }
        let mut attr_size = 0usize;
        InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attr_size);
        let mut attr_buf = vec![0u64; attr_size.div_ceil(8)];
        let attrs = attr_buf.as_mut_ptr() as *mut c_void;
        if InitializeProcThreadAttributeList(attrs, 1, 0, &mut attr_size) == 0 {
            return Err(last_error("InitializeProcThreadAttributeList failed"));
        }
        let attr_ok = UpdateProcThreadAttribute(
            attrs,
            0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            handles.as_ptr() as *const c_void,
            std::mem::size_of_val(&handles),
            null_mut(),
            null(),
        );
        if attr_ok == 0 {
            let error = last_error("UpdateProcThreadAttribute failed");
            DeleteProcThreadAttributeList(attrs);
            return Err(error);
        }
        let mut startup: STARTUPINFOEXW = std::mem::zeroed();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attrs;
        let mut info: PROCESS_INFORMATION = std::mem::zeroed();
        let created = CreateProcessAsUserW(
            token.as_raw_handle(),
            application_w.as_ptr(),
            command_line_w.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED
                | CREATE_UNICODE_ENVIRONMENT
                | CREATE_NO_WINDOW
                | EXTENDED_STARTUPINFO_PRESENT,
            env_block.as_ptr() as *const c_void,
            cwd_w.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        );
        let create_error = io::Error::last_os_error();
        DeleteProcThreadAttributeList(attrs);
        if created == 0 {
            return Err(format!("CreateProcessAsUser failed: {create_error}"));
        }
        let process = OwnedHandle::from_raw_handle(info.hProcess);
        let job = Job::create(limits);
        if let Some(job) = job.as_ref() {
            // Degrades like the unconfined path: no job, still isolated.
            let _ = job.assign(info.hProcess);
        }
        if ResumeThread(info.hThread) == u32::MAX {
            let error = last_error("ResumeThread failed");
            TerminateProcess(info.hProcess, 1);
            CloseHandle(info.hThread);
            return Err(error);
        }
        CloseHandle(info.hThread);
        // Close our copies of the child ends so EOF arrives when it exits.
        drop((stdin_child, stdout_child, stderr_child));
        Ok(LowIlSpawn {
            child: LowIlChild {
                process,
                _temp: temp,
            },
            stdin,
            stdout,
            stderr,
            job,
        })
    }
}
