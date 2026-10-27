//! OS-level isolation on Windows: run a command inside an AppContainer.
//!
//! An AppContainer process can open only files whose ACL grants its container
//! SID (or `ALL APPLICATION PACKAGES`, which already covers system locations
//! such as `C:\Windows` and `C:\Program Files`), and it has no network access
//! unless it holds the `internetClient` / `privateNetworkClientServer`
//! capabilities. [`grant`] adds inheritable ACEs for the container SID to the
//! directories a command may use — the workspace read/write, toolchains
//! read-only — so everything else (`~/.ssh`, other projects, credentials)
//! stays closed to it. [`restrict`] narrows the container's allow ACEs on
//! protected paths (`.git/hooks`, `.git/config`, `.env`, ...) inside a granted
//! tree — deny ACEs don't bind an AppContainer. No administrator rights are
//! needed.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{
    LocalFree, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetExplicitEntriesFromAclW, GetNamedSecurityInfoW, SetEntriesInAclW,
    SetNamedSecurityInfoW, ACCESS_MODE, EXPLICIT_ACCESS_W, GRANT_ACCESS, NO_MULTIPLE_TRUSTEE,
    SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_WELL_KNOWN_GROUP, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName, GetAppContainerFolderPath,
};
use windows_sys::Win32::Security::{
    AddAccessAllowedAceEx, AddAce, CreateWellKnownSid, EqualSid, FreeSid, GetAce, GetLengthSid,
    InitializeAcl, WinCapabilityInternetClientSid, WinCapabilityPrivateNetworkClientServerSid,
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, CONTAINER_INHERIT_ACE,
    DACL_SECURITY_INFORMATION, INHERITED_ACE, INHERIT_ONLY_ACE, OBJECT_INHERIT_ACE,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ALL_ACCESS, FILE_DELETE_CHILD, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, WRITE_DAC,
    WRITE_OWNER,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// AppContainer profile used for BugleCat commands.
pub const PROFILE_NAME: &str = "BugleCat.Sandbox";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const SE_GROUP_ENABLED: u32 = 0x0000_0004;
/// `HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)`.
const HRESULT_ALREADY_EXISTS: i32 = 0x8007_00B7_u32 as i32;

fn wide(text: impl AsRef<OsStr>) -> Vec<u16> {
    text.as_ref().encode_wide().chain(Some(0)).collect()
}

fn last_error(what: &str) -> io::Error {
    let error = io::Error::last_os_error();
    io::Error::new(error.kind(), format!("{what} failed: {error}"))
}

fn hresult(hr: i32, what: &str) -> io::Result<()> {
    if hr >= 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{what} failed: HRESULT {hr:#010x}"
        )))
    }
}

fn win32(code: u32, what: &str) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{what} failed: {}",
            io::Error::from_raw_os_error(code as i32)
        )))
    }
}

/// A per-user AppContainer profile (created on first use, then reused).
pub struct AppContainer {
    sid: PSID,
    temp_dir: PathBuf,
}

// The SID is owned by this struct, never mutated, and freed once in Drop.
unsafe impl Send for AppContainer {}
unsafe impl Sync for AppContainer {}

impl Drop for AppContainer {
    fn drop(&mut self) {
        unsafe {
            FreeSid(self.sid);
        }
    }
}

impl AppContainer {
    /// Create the profile `name`, or look it up when it already exists.
    pub fn open(name: &str) -> io::Result<Self> {
        let wname = wide(name);
        let mut sid: PSID = null_mut();
        // Several processes (CLI + GUI) or threads may create the profile at
        // the same moment; the losers get transient errors such as
        // E_UNEXPECTED until the winner finishes, so retry briefly.
        let mut attempt = 0;
        loop {
            let hr = unsafe {
                CreateAppContainerProfile(
                    wname.as_ptr(),
                    wname.as_ptr(),
                    wname.as_ptr(),
                    null(),
                    0,
                    &mut sid,
                )
            };
            if hr >= 0 {
                break;
            }
            if hr == HRESULT_ALREADY_EXISTS {
                hresult(
                    unsafe { DeriveAppContainerSidFromAppContainerName(wname.as_ptr(), &mut sid) },
                    "DeriveAppContainerSidFromAppContainerName",
                )?;
                break;
            }
            attempt += 1;
            if attempt >= 10 {
                hresult(hr, "CreateAppContainerProfile")?;
            }
            std::thread::sleep(std::time::Duration::from_millis(50 * attempt));
        }
        let mut container = AppContainer {
            sid,
            temp_dir: PathBuf::new(),
        };
        container.temp_dir = container.folder()?.join("Temp");
        std::fs::create_dir_all(&container.temp_dir)?;
        Ok(container)
    }

    /// The container's private, writable temp directory.
    pub fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    /// The container SID as `S-1-15-2-...`.
    pub fn sid_string(&self) -> io::Result<String> {
        unsafe {
            let mut text: *mut u16 = null_mut();
            if ConvertSidToStringSidW(self.sid, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let value = from_wide_ptr(text);
            LocalFree(text as _);
            Ok(value)
        }
    }

    fn folder(&self) -> io::Result<PathBuf> {
        let sid = wide(self.sid_string()?);
        unsafe {
            let mut path: *mut u16 = null_mut();
            hresult(
                GetAppContainerFolderPath(sid.as_ptr(), &mut path),
                "GetAppContainerFolderPath",
            )?;
            let value = from_wide_ptr(path);
            CoTaskMemFree(path as _);
            Ok(PathBuf::from(value))
        }
    }
}

/// The process-wide BugleCat container, opened on first use.
pub fn shared() -> io::Result<&'static AppContainer> {
    static CONTAINER: std::sync::OnceLock<Result<AppContainer, String>> =
        std::sync::OnceLock::new();
    CONTAINER
        .get_or_init(|| AppContainer::open(PROFILE_NAME).map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| io::Error::other(format!("AppContainer setup failed: {error}")))
}

unsafe fn from_wide_ptr(ptr: *const u16) -> String {
    let mut len = 0;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
}

/// What a granted directory tree allows the container to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    ReadWrite,
}

/// Rights that let the container delete, rename, or re-ACL an object, or
/// remove a directory's entries (NTFS honours a parent's `FILE_DELETE_CHILD`
/// even when the entry itself grants no `DELETE`).
pub const REMOVE_RIGHTS: u32 = DELETE | FILE_DELETE_CHILD | WRITE_DAC | WRITE_OWNER;
/// Read and execute, nothing else.
pub const READ_RIGHTS: u32 = FILE_GENERIC_READ | FILE_GENERIC_EXECUTE;

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_DENIED_ACE_TYPE: u8 = 1;

/// The container's allow rights on one object, split by where they apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContainerRights {
    /// Rights on the object itself.
    pub own: u32,
    /// Rights handed down to everything below a directory.
    pub inherited: u32,
}

fn access_mask(access: Access) -> u32 {
    match access {
        Access::Read => READ_RIGHTS,
        Access::ReadWrite => FILE_ALL_ACCESS,
    }
}

/// Whether `path` already carries an explicit, inheritable grant of at least
/// `access` for the container, so [`grant`] can skip re-propagating the ACL
/// through a large tree.
pub fn has_grant(container: &AppContainer, path: &Path, access: Access) -> io::Result<bool> {
    has_entry(
        container,
        path,
        GRANT_ACCESS,
        access_mask(access),
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )
}

/// Add an inheritable ACE for the container to `path` (and, by inheritance,
/// everything below it, including files that already exist). No integrity
/// label change is needed: an explicit grant to the container SID is enough
/// for the low-integrity container process to create and overwrite files.
pub fn grant(container: &AppContainer, path: &Path, access: Access) -> io::Result<()> {
    add_entry(
        container,
        path,
        GRANT_ACCESS,
        access_mask(access),
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )
}

/// The container's current rights on `path`, and whether any of them is
/// inherited from a parent directory (so a parent's grant still reaches it).
///
/// Deny ACEs can't take rights away from an AppContainer: its second access
/// check only collects what the package SID is *allowed*. Protected paths are
/// therefore shielded by narrowing the container's allow ACEs with
/// [`restrict`], never by adding deny ACEs.
pub fn container_rights(
    container: &AppContainer,
    path: &Path,
) -> io::Result<(ContainerRights, bool)> {
    with_dacl(path, |dacl| unsafe {
        let mut rights = ContainerRights::default();
        let mut from_parent = false;
        for ace in aces(dacl) {
            let header = *(ace as *const ACE_HEADER);
            if header.AceType != ACCESS_ALLOWED_ACE_TYPE || !ace_is_for(ace, container.sid) {
                continue;
            }
            let mask = (*(ace as *const ACCESS_ALLOWED_ACE)).Mask;
            let flags = header.AceFlags as u32;
            if flags & INHERIT_ONLY_ACE == 0 {
                rights.own |= mask;
            }
            if flags & (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) != 0 {
                rights.inherited |= mask;
            }
            if flags & INHERITED_ACE != 0 {
                from_parent = true;
            }
        }
        Ok((rights, from_parent))
    })
}

/// Replace the container's ACEs on `path` with exactly `rights`. When the
/// container's access came from a parent, the DACL is protected from
/// inheritance first: every other inherited ACE is kept as an explicit copy,
/// so users and groups keep exactly the access they had.
pub fn restrict(container: &AppContainer, path: &Path, rights: ContainerRights) -> io::Result<()> {
    let mut wpath = wide(path);
    let new_acl = with_dacl(path, |dacl| unsafe {
        let from_parent = aces(dacl).any(|ace| {
            let header = *(ace as *const ACE_HEADER);
            header.AceFlags as u32 & INHERITED_ACE != 0 && ace_is_for(ace, container.sid)
        });
        let sid_len = GetLengthSid(container.sid);
        let size = (*dacl).AclSize as u32
            + 2 * (std::mem::size_of::<ACCESS_ALLOWED_ACE>() as u32 + sid_len);
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let acl = buffer.as_mut_ptr() as *mut ACL;
        let revision = ((*dacl).AclRevision as u32).max(ACL_REVISION);
        if InitializeAcl(acl, size, revision) == 0 {
            return Err(last_error("InitializeAcl"));
        }
        // Deny ACEs first, then the rest, each in their original order, so
        // the flattened ACL stays canonical.
        for deny_pass in [true, false] {
            for ace in aces(dacl) {
                let header = *(ace as *const ACE_HEADER);
                let is_deny = header.AceType == ACCESS_DENIED_ACE_TYPE;
                let inherited = header.AceFlags as u32 & INHERITED_ACE != 0;
                if is_deny != deny_pass
                    || (inherited && !from_parent)
                    || ((header.AceType == ACCESS_ALLOWED_ACE_TYPE || is_deny)
                        && ace_is_for(ace, container.sid))
                {
                    continue;
                }
                let mut copy = std::slice::from_raw_parts(ace, header.AceSize as usize).to_vec();
                copy[1] &= !(INHERITED_ACE as u8);
                if AddAce(
                    acl,
                    revision,
                    u32::MAX,
                    copy.as_ptr() as _,
                    copy.len() as u32,
                ) == 0
                {
                    return Err(last_error("AddAce"));
                }
            }
        }
        for (mask, flags) in [
            (rights.own, 0),
            (
                rights.inherited,
                OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE | INHERIT_ONLY_ACE,
            ),
        ] {
            if mask != 0 && AddAccessAllowedAceEx(acl, revision, flags, mask, container.sid) == 0 {
                return Err(last_error("AddAccessAllowedAceEx"));
            }
        }
        Ok((buffer, from_parent))
    })?;
    let (mut buffer, protect) = new_acl;
    let mut info = DACL_SECURITY_INFORMATION;
    if protect {
        info |= PROTECTED_DACL_SECURITY_INFORMATION;
    }
    unsafe {
        win32(
            SetNamedSecurityInfoW(
                wpath.as_mut_ptr(),
                SE_FILE_OBJECT,
                info,
                null_mut(),
                null_mut(),
                buffer.as_mut_ptr() as *mut ACL,
                null(),
            ),
            "SetNamedSecurityInfoW",
        )
    }
}

/// Run `f` on `path`'s DACL; a missing (NULL) DACL is an error, since it would
/// grant everyone everything.
fn with_dacl<T>(path: &Path, f: impl FnOnce(*const ACL) -> io::Result<T>) -> io::Result<T> {
    let wpath = wide(path);
    unsafe {
        let mut dacl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        win32(
            GetNamedSecurityInfoW(
                wpath.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            ),
            "GetNamedSecurityInfoW",
        )?;
        let result = if dacl.is_null() {
            Err(io::Error::other(format!("{} has no DACL", path.display())))
        } else {
            f(dacl)
        };
        LocalFree(descriptor as _);
        result
    }
}

/// The ACEs of `acl`, as raw pointers to their headers.
unsafe fn aces(acl: *const ACL) -> impl Iterator<Item = *const u8> {
    (0..(*acl).AceCount as u32).filter_map(move |index| {
        let mut ace: *mut core::ffi::c_void = null_mut();
        (GetAce(acl, index, &mut ace) != 0).then_some(ace as *const u8)
    })
}

/// Whether an allow/deny ACE names `sid`. Both layouts put the SID right after
/// the header and mask.
unsafe fn ace_is_for(ace: *const u8, sid: PSID) -> bool {
    let header = *(ace as *const ACE_HEADER);
    matches!(
        header.AceType,
        ACCESS_ALLOWED_ACE_TYPE | ACCESS_DENIED_ACE_TYPE
    ) && EqualSid(
        &(*(ace as *const ACCESS_ALLOWED_ACE)).SidStart as *const u32 as PSID,
        sid,
    ) != 0
}

/// Whether `path`'s DACL holds an explicit `mode` ACE for the container that
/// covers `mask` and carries at least the `inheritance` flags.
fn has_entry(
    container: &AppContainer,
    path: &Path,
    mode: ACCESS_MODE,
    mask: u32,
    inheritance: u32,
) -> io::Result<bool> {
    let wpath = wide(path);
    unsafe {
        let mut dacl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        win32(
            GetNamedSecurityInfoW(
                wpath.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            ),
            "GetNamedSecurityInfoW",
        )?;
        let mut count = 0u32;
        let mut entries: *mut EXPLICIT_ACCESS_W = null_mut();
        let listed = GetExplicitEntriesFromAclW(dacl, &mut count, &mut entries);
        let mut found = false;
        if listed == 0 && count > 0 && !entries.is_null() {
            for entry in std::slice::from_raw_parts(entries, count as usize) {
                if entry.grfAccessMode == mode
                    && entry.Trustee.TrusteeForm == TRUSTEE_IS_SID
                    && entry.grfInheritance & inheritance == inheritance
                    && entry.grfAccessPermissions & mask == mask
                    && EqualSid(entry.Trustee.ptstrName as PSID, container.sid) != 0
                {
                    found = true;
                    break;
                }
            }
        }
        if !entries.is_null() {
            LocalFree(entries as _);
        }
        LocalFree(descriptor as _);
        win32(listed, "GetExplicitEntriesFromAclW")?;
        Ok(found)
    }
}

/// Merge one explicit ACE for the container into `path`'s DACL.
fn add_entry(
    container: &AppContainer,
    path: &Path,
    mode: ACCESS_MODE,
    mask: u32,
    inheritance: u32,
) -> io::Result<()> {
    let mut wpath = wide(path);
    unsafe {
        let mut old_dacl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        win32(
            GetNamedSecurityInfoW(
                wpath.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut old_dacl,
                null_mut(),
                &mut descriptor,
            ),
            "GetNamedSecurityInfoW",
        )?;
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: mask,
            grfAccessMode: mode,
            grfInheritance: inheritance,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_WELL_KNOWN_GROUP,
                ptstrName: container.sid as *mut u16,
            },
        };
        let mut new_dacl: *mut ACL = null_mut();
        let merged = SetEntriesInAclW(1, &entry, old_dacl, &mut new_dacl);
        LocalFree(descriptor as _);
        win32(merged, "SetEntriesInAclW")?;
        let applied = SetNamedSecurityInfoW(
            wpath.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            new_dacl,
            null(),
        );
        LocalFree(new_dacl as _);
        win32(applied, "SetNamedSecurityInfoW")?;
    }
    Ok(())
}

/// A process running inside the AppContainer, created suspended.
pub struct IsolatedChild {
    process: Arc<OwnedHandle>,
    thread: Option<OwnedHandle>,
    pid: u32,
    pub stdin: Option<File>,
    pub stdout: Option<File>,
    pub stderr: Option<File>,
}

impl IsolatedChild {
    pub fn id(&self) -> u32 {
        self.pid
    }

    /// Start the (suspended) process.
    pub fn resume(&mut self) -> io::Result<()> {
        if let Some(thread) = self.thread.take() {
            if unsafe { ResumeThread(thread.as_raw_handle() as HANDLE) } == u32::MAX {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// Exit code once the process has finished, without blocking.
    pub fn try_wait(&self) -> io::Result<Option<i32>> {
        let handle = self.process.as_raw_handle() as HANDLE;
        if unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
            return Ok(None);
        }
        exit_code(handle).map(Some)
    }

    /// Wait for the process on a blocking thread.
    pub async fn wait(&self) -> io::Result<i32> {
        let process = self.process.clone();
        tokio::task::spawn_blocking(move || {
            let handle = process.as_raw_handle() as HANDLE;
            unsafe { WaitForSingleObject(handle, INFINITE) };
            exit_code(handle)
        })
        .await
        .map_err(io::Error::other)?
    }

    pub fn kill(&self) {
        unsafe {
            TerminateProcess(self.process.as_raw_handle() as HANDLE, 1);
        }
    }
}

fn exit_code(handle: HANDLE) -> io::Result<i32> {
    let mut code = 0u32;
    if unsafe { GetExitCodeProcess(handle, &mut code) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(code as i32)
}

/// An anonymous pipe whose `child` end is inheritable and `parent` end is not.
struct Pipe {
    child: OwnedHandle,
    parent: OwnedHandle,
}

fn pipe(child_reads: bool) -> io::Result<Pipe> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write): (HANDLE, HANDLE) = (null_mut(), null_mut());
    unsafe {
        if CreatePipe(&mut read, &mut write, &attributes, 0) == 0 {
            return Err(last_error("CreatePipe"));
        }
        let (read, write) = (
            OwnedHandle::from_raw_handle(read as RawHandle),
            OwnedHandle::from_raw_handle(write as RawHandle),
        );
        let (child, parent) = if child_reads {
            (read, write)
        } else {
            (write, read)
        };
        if SetHandleInformation(parent.as_raw_handle() as HANDLE, HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(last_error("SetHandleInformation"));
        }
        Ok(Pipe { child, parent })
    }
}

fn well_known_sid(kind: i32) -> io::Result<Vec<u8>> {
    let mut size = 68u32; // SECURITY_MAX_SID_SIZE
    let mut buffer = vec![0u8; size as usize];
    if unsafe { CreateWellKnownSid(kind, null_mut(), buffer.as_mut_ptr() as PSID, &mut size) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(size as usize);
    Ok(buffer)
}

/// Owns a `PROC_THREAD_ATTRIBUTE_LIST` buffer.
struct AttributeList(Vec<u8>);

impl AttributeList {
    fn new(count: u32) -> io::Result<Self> {
        let mut size = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size);
        }
        let mut buffer = vec![0u8; size];
        if unsafe {
            InitializeProcThreadAttributeList(buffer.as_mut_ptr() as _, count, 0, &mut size)
        } == 0
        {
            return Err(last_error("InitializeProcThreadAttributeList"));
        }
        Ok(AttributeList(buffer))
    }

    fn as_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.0.as_mut_ptr() as _
    }

    /// `value` must outlive the process creation that uses this list.
    unsafe fn set<T>(&mut self, attribute: usize, value: *const T, size: usize) -> io::Result<()> {
        if UpdateProcThreadAttribute(
            self.as_ptr(),
            0,
            attribute,
            value as _,
            size,
            null_mut(),
            null(),
        ) == 0
        {
            return Err(last_error("UpdateProcThreadAttribute"));
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
    }
}

/// Start `command_line` (run by `application`) suspended inside `container`,
/// with piped stdio, working directory `cwd`, and exactly the variables in
/// `env`. `network` grants the internet and private-network capabilities.
/// Call [`IsolatedChild::resume`] once it has been placed in a job.
pub fn spawn(
    container: &AppContainer,
    application: &Path,
    command_line: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    network: bool,
) -> io::Result<IsolatedChild> {
    let stdin = pipe(true)?;
    let stdout = pipe(false)?;
    let stderr = pipe(false)?;

    let capability_sids = if network {
        vec![
            well_known_sid(WinCapabilityInternetClientSid)?,
            well_known_sid(WinCapabilityPrivateNetworkClientServerSid)?,
        ]
    } else {
        Vec::new()
    };
    let mut capabilities: Vec<SID_AND_ATTRIBUTES> = capability_sids
        .iter()
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid.as_ptr() as PSID,
            Attributes: SE_GROUP_ENABLED,
        })
        .collect();
    let security = SECURITY_CAPABILITIES {
        AppContainerSid: container.sid,
        Capabilities: if capabilities.is_empty() {
            null_mut()
        } else {
            capabilities.as_mut_ptr()
        },
        CapabilityCount: capabilities.len() as u32,
        Reserved: 0,
    };
    // Only the three stdio handles may be inherited, even if other threads
    // are creating inheritable handles at the same time.
    let inherited: [HANDLE; 3] = [
        stdin.child.as_raw_handle() as HANDLE,
        stdout.child.as_raw_handle() as HANDLE,
        stderr.child.as_raw_handle() as HANDLE,
    ];
    let mut attributes = AttributeList::new(2)?;
    unsafe {
        attributes.set(
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &security,
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
        )?;
        attributes.set(
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
            inherited.as_ptr(),
            std::mem::size_of_val(&inherited),
        )?;
    }

    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = inherited[0];
    startup.StartupInfo.hStdOutput = inherited[1];
    startup.StartupInfo.hStdError = inherited[2];
    startup.lpAttributeList = attributes.as_ptr();

    // CreateProcess maps the container's private data directory from
    // LOCALAPPDATA and fails with ERROR_ENVVAR_NOT_FOUND without it.
    let mut env = env.clone();
    if !env
        .keys()
        .any(|key| key.eq_ignore_ascii_case("LOCALAPPDATA"))
    {
        if let Some(value) = std::env::var_os("LOCALAPPDATA") {
            env.insert("LOCALAPPDATA".into(), value.to_string_lossy().into_owned());
        }
    }
    let mut env_block: Vec<u16> = Vec::new();
    for (key, value) in &env {
        env_block.extend(OsStr::new(&format!("{key}={value}")).encode_wide());
        env_block.push(0);
    }
    env_block.push(0);

    let application = wide(application);
    let mut command_line = wide(command_line);
    let cwd = wide(cwd);
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT,
            env_block.as_ptr() as _,
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    };
    if created == 0 {
        return Err(last_error("CreateProcessW"));
    }
    // The child's ends are closed here when the pipes drop; the child holds
    // its own inherited copies.
    unsafe {
        Ok(IsolatedChild {
            process: Arc::new(OwnedHandle::from_raw_handle(info.hProcess as RawHandle)),
            thread: Some(OwnedHandle::from_raw_handle(info.hThread as RawHandle)),
            pid: info.dwProcessId,
            stdin: Some(File::from(stdin.parent)),
            stdout: Some(File::from(stdout.parent)),
            stderr: Some(File::from(stderr.parent)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    const TEST_PROFILE: &str = "BugleCat.SandboxTest";

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ncx-appcontainer-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Run `command` through cmd.exe in the test container; returns
    /// (exit code, stdout + stderr).
    fn run(container: &AppContainer, cwd: &Path, command: &str, network: bool) -> (i32, String) {
        let system_root = std::env::var("SYSTEMROOT").unwrap_or_else(|_| r"C:\Windows".into());
        let comspec = PathBuf::from(&system_root).join("System32").join("cmd.exe");
        let temp = container.temp_dir().display().to_string();
        let env = BTreeMap::from([
            ("SYSTEMROOT".to_string(), system_root.clone()),
            (
                "PATH".to_string(),
                format!(r"{system_root}\System32;{system_root}"),
            ),
            ("TEMP".to_string(), temp.clone()),
            ("TMP".to_string(), temp),
        ]);
        let mut child = spawn(
            container,
            &comspec,
            &format!("cmd.exe /D /S /C \"{command}\""),
            cwd,
            &env,
            network,
        )
        .unwrap();
        child.stdin.take();
        child.resume().unwrap();
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        let out = String::from_utf8_lossy(&bytes).into_owned();
        let code = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(child.wait())
            .unwrap();
        (code, out)
    }

    #[test]
    fn container_can_write_only_where_granted() {
        let container = AppContainer::open(TEST_PROFILE).unwrap();
        let workspace = temp_dir("ws");
        let outside = temp_dir("outside");
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(workspace.join("src").join("existing.txt"), "old").unwrap();
        assert!(!has_grant(&container, &workspace, Access::Read).unwrap());
        grant(&container, &workspace, Access::ReadWrite).unwrap();
        assert!(has_grant(&container, &workspace, Access::ReadWrite).unwrap());
        assert!(has_grant(&container, &workspace, Access::Read).unwrap());
        assert!(!has_grant(&container, &outside, Access::Read).unwrap());

        let (code, out) = run(&container, &workspace, "echo inside> inside.txt", false);
        assert_eq!(code, 0, "{out}");
        assert!(workspace.join("inside.txt").exists(), "{out}");
        // Files that existed before the grant are writable too (inherited ACE).
        let (code, out) = run(&container, &workspace, r"echo new> src\existing.txt", false);
        assert_eq!(code, 0, "{out}");
        assert_eq!(
            std::fs::read_to_string(workspace.join("src").join("existing.txt"))
                .unwrap()
                .trim(),
            "new"
        );

        let target = outside.join("escaped.txt");
        let (code, _) = run(
            &container,
            &workspace,
            &format!("echo escaped> \"{}\"", target.display()),
            false,
        );
        assert_ne!(code, 0);
        assert!(!target.exists(), "write outside the granted tree must fail");
        let _ = std::fs::remove_dir_all(workspace);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn container_cannot_read_ungranted_files() {
        let container = AppContainer::open(TEST_PROFILE).unwrap();
        let workspace = temp_dir("ws-read");
        let secrets = temp_dir("secrets");
        std::fs::write(secrets.join("id_rsa"), "TOPSECRET").unwrap();
        grant(&container, &workspace, Access::ReadWrite).unwrap();

        let (_, out) = run(
            &container,
            &workspace,
            &format!("type \"{}\"", secrets.join("id_rsa").display()),
            false,
        );
        assert!(!out.contains("TOPSECRET"), "{out}");

        // A read-only grant makes the same file readable but not writable.
        grant(&container, &secrets, Access::Read).unwrap();
        let (code, out) = run(
            &container,
            &workspace,
            &format!("type \"{}\"", secrets.join("id_rsa").display()),
            false,
        );
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("TOPSECRET"), "{out}");
        let (code, _) = run(
            &container,
            &workspace,
            &format!("echo x> \"{}\"", secrets.join("new.txt").display()),
            false,
        );
        assert_ne!(code, 0);
        assert!(!secrets.join("new.txt").exists());
        let _ = std::fs::remove_dir_all(workspace);
        let _ = std::fs::remove_dir_all(secrets);
    }

    #[test]
    fn container_without_network_capability_cannot_connect() {
        let container = AppContainer::open(TEST_PROFILE).unwrap();
        let workspace = temp_dir("ws-net");
        grant(&container, &workspace, Access::ReadWrite).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let system_root = std::env::var("SYSTEMROOT").unwrap_or_else(|_| r"C:\Windows".into());
        let curl = PathBuf::from(system_root).join("System32").join("curl.exe");
        if !curl.exists() {
            return;
        }
        let (code, out) = run(
            &container,
            &workspace,
            &format!("curl.exe -s -m 5 http://127.0.0.1:{port}/"),
            false,
        );
        assert_ne!(code, 0, "{out}");
        assert!(
            listener.accept().is_err(),
            "no connection may reach the listener"
        );
        let _ = std::fs::remove_dir_all(workspace);
    }
}
