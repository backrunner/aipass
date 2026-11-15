//! Readiness from a signed AppImage must belong to its live runtime process tree.
pub(super) fn matches(
    reported: &str,
    spawned: u32,
    descendants: bool,
    mut parent: impl FnMut(u32) -> Option<u32>,
) -> bool {
    let Ok(mut pid) = reported.parse::<u32>() else {
        return false;
    };
    if pid == 0 || spawned == 0 {
        return false;
    }
    if !descendants {
        return pid == spawned;
    }
    for _ in 0..32 {
        if pid == spawned {
            return true;
        }
        let Some(next) = parent(pid).filter(|next| *next > 1 && *next != pid) else {
            return false;
        };
        pid = next;
    }
    false
}

pub(super) fn parent(status: &str, expected_pid: u32, uid: u32) -> Option<u32> {
    let field = |name| status.lines().find_map(|line| line.strip_prefix(name));
    if field("Pid:\t")?.trim().parse::<u32>().ok()? != expected_pid {
        return None;
    }
    let state = field("State:\t")?.split_whitespace().next()?;
    if matches!(state, "Z" | "X" | "x") {
        return None;
    }
    let mut ids = field("Uid:\t")?.split_whitespace();
    for _ in 0..4 {
        if ids.next()?.parse::<u32>().ok()? != uid {
            return None;
        }
    }
    if ids.next().is_some() {
        return None;
    }
    field("PPid:\t")?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_identity_matches_a_live_child_and_rejects_its_ancestor() {
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let read_parent = |pid| {
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
            parent(&status, pid, rustix::process::geteuid().as_raw())
        };
        let descendant = matches(
            &child.id().to_string(),
            std::process::id(),
            true,
            read_parent,
        );
        let ancestor = matches(
            &std::process::id().to_string(),
            child.id(),
            true,
            read_parent,
        );
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(descendant);
        assert!(!ancestor);
    }

    #[test]
    fn readiness_accepts_only_the_spawned_process_or_its_allowed_descendants() {
        let chain = |pid| match pid {
            30 => Some(20),
            20 => Some(10),
            40 => Some(1),
            _ => None,
        };
        assert!(matches("10", 10, false, chain));
        assert!(matches("30", 10, true, chain));
        assert!(!matches("30", 10, false, chain));
        assert!(!matches("40", 10, true, chain));
        assert!(!matches("50", 10, true, chain));
        assert!(!matches("0", 0, true, chain));
        assert!(!matches("10\n", 10, true, chain));
        assert!(!matches("30", 10, true, |_| Some(30)));
        assert!(!matches("30", 10, true, |pid| Some(50 - pid)));
    }

    #[test]
    fn proc_identity_rejects_foreign_users_dead_processes_and_malformed_status() {
        let status = "Name:\tAIPass (test)\nState:\tS (sleeping)\nPid:\t30\nPPid:\t20\nUid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parent(status, 30, 1000), Some(20));
        assert_eq!(parent(status, 31, 1000), None);
        assert_eq!(parent(status, 30, 1001), None);
        assert_eq!(
            parent(&status.replace("S (sleeping)", "Z (zombie)"), 30, 1000),
            None
        );
        assert_eq!(
            parent(
                &status.replace("1000\t1000\t1000\t1000", "1000\t0\t1000\t1000"),
                30,
                1000
            ),
            None
        );
        assert_eq!(parent(&status.replace("Uid:", "Gid:"), 30, 1000), None);
        assert_eq!(
            parent(&status.replace("PPid:\t20", "PPid:\tinvalid"), 30, 1000),
            None
        );
    }
}
