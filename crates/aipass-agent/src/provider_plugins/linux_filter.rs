//! A syscall allowlist for ordinary single-process Rust and Go executables.
//! bubblewrap installs it after mounting the read-only root and before exec.
use anyhow::Result;
use libc::*;
use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Seek, Write},
};

pub(super) fn descriptor() -> Result<File> {
    let calls = [
        SYS_read,
        SYS_write,
        SYS_readv,
        SYS_writev,
        SYS_pread64,
        SYS_close,
        SYS_fstat,
        SYS_newfstatat,
        SYS_statx,
        SYS_lseek,
        SYS_openat,
        SYS_readlinkat,
        SYS_fcntl,
        SYS_getcwd,
        SYS_getdents64,
        SYS_mmap,
        SYS_mprotect,
        SYS_munmap,
        SYS_mremap,
        SYS_madvise,
        SYS_brk,
        SYS_clock_gettime,
        SYS_clock_nanosleep,
        SYS_gettimeofday,
        SYS_nanosleep,
        SYS_sched_yield,
        SYS_sched_getaffinity,
        SYS_futex,
        SYS_set_tid_address,
        SYS_set_robust_list,
        SYS_rseq,
        SYS_rt_sigaction,
        SYS_rt_sigprocmask,
        SYS_sigaltstack,
        SYS_rt_sigreturn,
        SYS_tgkill,
        SYS_getpid,
        SYS_getppid,
        SYS_gettid,
        SYS_getuid,
        SYS_geteuid,
        SYS_getgid,
        SYS_getegid,
        SYS_prlimit64,
        SYS_getrusage,
        // bubblewrap's PID 1 inherits this filter and calls wait() to report
        // the worker's exit status. wait4 only reaps an existing child.
        SYS_wait4,
        SYS_uname,
        SYS_getrandom,
        SYS_execve,
        SYS_exit,
        SYS_exit_group,
        SYS_epoll_create1,
        SYS_epoll_ctl,
        SYS_epoll_pwait,
        SYS_eventfd2,
        SYS_ppoll,
        SYS_pipe2,
        SYS_dup,
        SYS_dup3,
        SYS_prctl,
        SYS_restart_syscall,
        SYS_faccessat,
    ];
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> =
        calls.into_iter().map(|n| (n, vec![])).collect();
    #[cfg(target_arch = "x86_64")]
    for n in [
        SYS_arch_prctl,
        SYS_readlink,
        SYS_open,
        SYS_access,
        SYS_stat,
        SYS_lstat,
        SYS_poll,
        SYS_epoll_wait,
        SYS_getrlimit,
        SYS_dup2,
    ] {
        rules.insert(n, vec![]);
    }
    // clone3 receives an opaque struct. ENOSYS makes libc fall back to clone,
    // whose flags the kernel filter can check before allowing a thread.
    let flag = CLONE_THREAD as u64;
    let thread = SeccompCondition::new(
        0,
        SeccompCmpArgLen::Qword,
        SeccompCmpOp::MaskedEq(flag),
        flag,
    )?;
    rules.insert(SYS_clone, vec![SeccompRule::new(vec![thread])?]);
    let program: BpfProgram = SeccompFilter::new(
        rules,
        SeccompAction::Errno(ENOSYS as u32),
        SeccompAction::Allow,
        std::env::consts::ARCH.try_into()?,
    )?
    .try_into()?;
    let mut file = tempfile::tempfile()?;
    for instruction in program {
        file.write_all(&instruction.code.to_ne_bytes())?;
        file.write_all(&[instruction.jt, instruction.jf])?;
        file.write_all(&instruction.k.to_ne_bytes())?;
    }
    file.rewind()?;
    rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::empty())?;
    Ok(file)
}
