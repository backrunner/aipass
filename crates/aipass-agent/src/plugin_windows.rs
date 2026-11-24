//! This short-lived broker creates a capability-free LPAC child and binds it to a
//! single-process job at creation. No guest instruction runs before either boundary.
use anyhow::{ensure, Context, Result};
use std::{
    ffi::{c_void, OsStr},
    mem::{size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{
        DuplicateHandle, GetLastError, LocalFree, DUPLICATE_SAME_ACCESS, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    },
    Security::{
        Authorization::*,
        FreeSid,
        Isolation::{CreateAppContainerProfile, DeleteAppContainerProfile},
        DACL_SECURITY_INFORMATION, PSID, SECURITY_CAPABILITIES,
    },
    Storage::FileSystem::{FILE_GENERIC_EXECUTE, FILE_GENERIC_READ},
    System::{Console::*, JobObjects::*, Threading::*},
};
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn worker_environment(stage: &Path) -> Result<Vec<u16>> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
    use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;

    let mut buffer = vec![0; 32_768];
    let count = unsafe { GetSystemWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    ensure!(
        count > 0 && (count as usize) < buffer.len(),
        "Cannot resolve Windows system root"
    );
    let windows = PathBuf::from(OsString::from_wide(&buffer[..count as usize]));
    let system = windows.join("System32");
    // CreateProcessW needs a Unicode environment to initialize an AppContainer.
    // Seed only system and staging paths. Never pass the broker environment.
    let mut block = Vec::new();
    for (key, path) in [
        ("APPDATA", stage),
        ("LOCALAPPDATA", stage),
        ("PATH", system.as_path()),
        ("SystemRoot", windows.as_path()),
        ("TEMP", stage),
        ("TMP", stage),
        ("USERPROFILE", stage),
        ("windir", windows.as_path()),
    ] {
        block.extend(key.encode_utf16());
        block.push(b'=' as u16);
        block.extend(wide(path.as_os_str()));
    }
    block.push(0);
    Ok(block)
}

fn check(ok: i32) -> Result<()> {
    if ok == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

struct Profile {
    name: Vec<u16>,
    sid: PSID,
}
impl Profile {
    fn create() -> Result<Self> {
        let name = wide(OsStr::new(&format!(
            "AIPass.Plugin.{}",
            uuid::Uuid::new_v4().simple()
        )));
        let mut sid = null_mut();
        // A derived SID alone does not create the profile Windows needs to
        // initialize a new AppContainer. Each launch owns a distinct profile.
        let hr = unsafe {
            CreateAppContainerProfile(
                name.as_ptr(),
                name.as_ptr(),
                name.as_ptr(),
                null(),
                0,
                &mut sid,
            )
        };
        ensure!(
            hr == 0 && !sid.is_null(),
            "Cannot create plugin AppContainer profile ({hr:#x})"
        );
        Ok(Self { name, sid })
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        unsafe {
            FreeSid(self.sid);
            let result = DeleteAppContainerProfile(self.name.as_ptr());
            if result < 0 {
                eprintln!("Cannot remove plugin AppContainer profile ({result:#x})");
            }
        }
    }
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn allow_read_execute(path: &Path, sid: PSID) -> Result<()> {
    let path = wide(path.as_os_str());
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    unsafe {
        let result = GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        );
        ensure!(result == 0, "Cannot read staged package ACL ({result})");
        let _descriptor = Local(descriptor);
        ensure!(!acl.is_null(), "Stage requires a DACL");
        let grant = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_GENERIC_READ | FILE_GENERIC_EXECUTE,
            grfAccessMode: GRANT_ACCESS,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                ptstrName: sid.cast(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut updated = null_mut();
        let result = SetEntriesInAclW(1, &grant, acl, &mut updated);
        ensure!(result == 0, "Cannot prepare package ACL ({result})");
        let _updated = Local(updated.cast());
        let result = SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            updated,
            null(),
        );
        ensure!(result == 0, "Cannot apply package ACL ({result})");
    }
    Ok(())
}

struct Attributes {
    storage: Vec<usize>,
}
impl Attributes {
    fn new(count: u32) -> Result<Self> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), count, 0, &mut size);
        }
        ensure!(size > 0, "Process attributes unavailable");
        let mut storage = vec![0; size.div_ceil(size_of::<usize>())];
        unsafe {
            check(InitializeProcThreadAttributeList(
                storage.as_mut_ptr().cast(),
                count,
                0,
                &mut size,
            ))?;
        }
        Ok(Self { storage })
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    // Attribute values are borrowed by Windows until CreateProcessW returns.
    unsafe fn set<T>(&mut self, id: u32, value: &T) -> Result<()> {
        unsafe {
            check(UpdateProcThreadAttribute(
                self.ptr(),
                0,
                id as usize,
                (value as *const T).cast(),
                size_of_val(value),
                null_mut(),
                null(),
            ))
        }
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe {
            DeleteProcThreadAttributeList(self.ptr());
        }
    }
}

fn std_handle(id: STD_HANDLE) -> Result<OwnedHandle> {
    unsafe {
        let source = GetStdHandle(id);
        ensure!(
            !source.is_null() && source != INVALID_HANDLE_VALUE,
            "Missing plugin pipe"
        );
        let process = GetCurrentProcess();
        let mut copy = null_mut();
        check(DuplicateHandle(
            process,
            source,
            process,
            &mut copy,
            0,
            1,
            DUPLICATE_SAME_ACCESS,
        ))?;
        Ok(OwnedHandle::from_raw_handle(copy))
    }
}
pub(crate) fn run(path: &Path) -> Result<()> {
    let stage = tempfile::tempdir_in(path.parent().context("Missing package directory")?)?;
    let image_path = stage.path().join("provider.exe");
    ensure!(
        std::fs::metadata(path)?.len() <= 64 * 1024 * 1024,
        "Plugin exceeds size limit"
    );
    std::fs::copy(path, &image_path)?;
    let path = image_path.as_path();
    let parent = stage.path();
    let agent = std::env::var_os("AIPASS_PLUGIN_AGENT_PID")
        .map(|raw| -> Result<OwnedHandle> {
            let pid = raw.to_str().context("Invalid Agent PID")?.parse()?;
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            ensure!(!handle.is_null(), "Agent exited before sandbox startup");
            Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
        })
        .transpose()?;
    let profile = Profile::create()?;
    allow_read_execute(parent, profile.sid)?;
    allow_read_execute(path, profile.sid)?;
    let image = wide(path.as_os_str());
    let cwd = wide(parent.as_os_str());
    let caps = SECURITY_CAPABILITIES {
        AppContainerSid: profile.sid,
        ..Default::default()
    };
    let handles = [
        std_handle(STD_INPUT_HANDLE)?,
        std_handle(STD_OUTPUT_HANDLE)?,
        std_handle(STD_ERROR_HANDLE)?,
    ];
    let stdio = handles.each_ref().map(|h| h.as_raw_handle());
    unsafe {
        let raw_job = CreateJobObjectW(null(), null());
        ensure!(
            !raw_job.is_null(),
            "Cannot create plugin job ({})",
            GetLastError()
        );
        let job = OwnedHandle::from_raw_handle(raw_job);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_PROCESS_TIME;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.BasicLimitInformation.PerProcessUserTimeLimit = 600_000_000;
        limits.ProcessMemoryLimit = 256 * 1024 * 1024;
        check(SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of_val(&limits) as u32,
        ))?;
        let jobs: [HANDLE; 1] = [job.as_raw_handle()];
        let lpac = 1u32; // PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT
        let mut attrs = Attributes::new(4)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &caps)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY, &lpac)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &stdio)?;
        attrs.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of_val(&startup) as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        [
            startup.StartupInfo.hStdInput,
            startup.StartupInfo.hStdOutput,
            startup.StartupInfo.hStdError,
        ] = stdio;
        startup.lpAttributeList = attrs.ptr();
        let mut info = PROCESS_INFORMATION::default();
        let env = worker_environment(parent)?;
        check(CreateProcessW(
            image.as_ptr(),
            null_mut(),
            null(),
            null(),
            1,
            CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            env.as_ptr().cast(),
            cwd.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        ))
        .context("Cannot launch the staged plugin inside its AppContainer")?;
        let process = OwnedHandle::from_raw_handle(info.hProcess);
        let _thread = OwnedHandle::from_raw_handle(info.hThread);
        let mut wait = vec![process.as_raw_handle()];
        if let Some(agent) = &agent {
            wait.push(agent.as_raw_handle());
        }
        if WaitForMultipleObjects(wait.len() as u32, wait.as_ptr(), 0, 1_200_000) != WAIT_OBJECT_0 {
            check(TerminateJobObject(job.as_raw_handle(), 1)).context("Cannot stop plugin job")?;
            ensure!(
                WaitForSingleObject(process.as_raw_handle(), 5_000) == WAIT_OBJECT_0,
                "Plugin failed to stop"
            );
            anyhow::bail!("Agent exited or plugin job timed out");
        }
        let mut code = 0;
        check(GetExitCodeProcess(process.as_raw_handle(), &mut code))?;
        ensure!(code == 0, "Plugin exited with status {code:#x}");
    }
    Ok(())
}
