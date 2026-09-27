use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;

#[cfg(unix)]
static SIGINT_RECEIVED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn record_sigint(_: libc::c_int) {
    SIGINT_RECEIVED.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
struct SigintHandler(libc::sigaction);

#[cfg(unix)]
impl SigintHandler {
    fn install() -> Result<Self> {
        SIGINT_RECEIVED.store(false, Ordering::Relaxed);
        // SAFETY: sigaction is initialized before use and the previous action is returned here.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = record_sigint as *const () as usize;
        action.sa_flags = 0;
        // SAFETY: action is writable and uses an empty mask for this plain signal handler.
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        // SAFETY: previous is initialized by sigaction when installation succeeds.
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        if unsafe { libc::sigaction(libc::SIGINT, &action, &mut previous) } == -1 {
            return Err(std::io::Error::last_os_error())
                .context("could not install the detached worker interrupt handler");
        }
        Ok(Self(previous))
    }
}

#[cfg(unix)]
impl Drop for SigintHandler {
    fn drop(&mut self) {
        // SAFETY: self.0 is the prior handler returned by sigaction.
        unsafe { libc::sigaction(libc::SIGINT, &self.0, std::ptr::null_mut()) };
    }
}

#[cfg(unix)]
fn wait_for_detached_worker(child: &mut Child) -> Result<ExitStatus> {
    let process_group: libc::pid_t = child
        .id()
        .try_into()
        .context("worker PID is out of range")?;
    loop {
        if let Some(status) = child
            .try_wait()
            .context("could not wait for detached worker")?
        {
            return Ok(status);
        }
        if SIGINT_RECEIVED.swap(false, Ordering::Relaxed) {
            // SAFETY: the worker is the leader of its new session and process group.
            if unsafe { libc::kill(-process_group, libc::SIGINT) } == -1 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    // SAFETY: terminate the isolated worker group if forwarding failed.
                    unsafe { libc::kill(-process_group, libc::SIGKILL) };
                    let _ = child.wait();
                    return Err(error).context("could not forward interrupt to detached worker");
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub struct Herdr {
    workspace_id: String,
    open_workspaces: HashMap<PathBuf, String>,
}

impl Herdr {
    pub fn is_active_context() -> bool {
        #[cfg(unix)]
        {
            std::env::var("HERDR_ENV").as_deref() == Ok("1")
                && std::env::var("HERDR_WORKSPACE_ID")
                    .is_ok_and(|workspace_id| !workspace_id.is_empty())
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    pub fn run_lifecycle_in_detached_session() -> Result<ExitStatus> {
        let executable = std::env::current_exe().context("could not locate wt executable")?;
        let mut worker = Command::new(executable);
        worker
            .arg("--internal-herdr-detached")
            .args(std::env::args_os().skip(1));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: setsid is async-signal-safe and the closure runs in the fresh worker.
            unsafe {
                worker.pre_exec(|| {
                    // SAFETY: this worker is a child process and is not a process-group leader.
                    if libc::setsid() == -1 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
        }
        #[cfg(unix)]
        {
            let _sigint_handler = SigintHandler::install()?;
            let mut child = worker
                .spawn()
                .context("could not start detached wt lifecycle command")?;
            wait_for_detached_worker(&mut child)
        }
        #[cfg(not(unix))]
        {
            worker
                .status()
                .context("could not start detached wt lifecycle command")
        }
    }

    pub fn active(repo_root: &Path) -> Result<Option<Self>> {
        if std::env::var("HERDR_ENV").as_deref() != Ok("1") {
            return Ok(None);
        }
        let Some(workspace_id) = std::env::var("HERDR_WORKSPACE_ID")
            .ok()
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };
        let output = Command::new("herdr")
            .current_dir(repo_root)
            .args(["worktree", "list", "--workspace", &workspace_id])
            .output()
            .context("could not run herdr worktree list")?;
        if !output.status.success() {
            bail!(
                "herdr worktree list failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let list: Value = serde_json::from_slice(&output.stdout)
            .context("herdr worktree list returned invalid JSON")?;
        let result = list
            .get("result")
            .context("herdr worktree list response has no result")?;
        let source = result
            .get("source")
            .context("herdr worktree list response has no result.source")?;
        let source_id = source
            .get("source_workspace_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .context("herdr worktree list response has no source workspace ID")?;
        let reported_root = source
            .get("repo_root")
            .and_then(Value::as_str)
            .filter(|root| !root.is_empty())
            .context("herdr worktree list response has no repository root")?;
        let expected_root = repo_root.canonicalize().with_context(|| {
            format!("could not resolve repository root {}", repo_root.display())
        })?;
        let reported_root = Path::new(reported_root)
            .canonicalize()
            .context("could not resolve repository root from herdr worktree list")?;
        if reported_root != expected_root {
            bail!(
                "herdr workspace repository root does not match Git primary root: {}",
                reported_root.display()
            );
        }

        let worktrees = result
            .get("worktrees")
            .and_then(Value::as_array)
            .context("herdr worktree list response has no result.worktrees array")?;
        let mut open_workspaces = HashMap::new();
        for worktree in worktrees {
            let path = worktree
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .context("herdr worktree list response has a worktree without a path")?;
            let workspace_id = match worktree.get("open_workspace_id") {
                None | Some(Value::Null) => None,
                Some(Value::String(id)) if id.is_empty() => None,
                Some(Value::String(id)) => Some(id.as_str()),
                Some(_) => bail!(
                    "herdr worktree list response has an invalid open workspace ID for {path}"
                ),
            };
            let Some(workspace_id) = workspace_id else {
                continue;
            };
            let path = PathBuf::from(path)
                .canonicalize()
                .with_context(|| format!("could not resolve Herdr worktree path {path}"))?;
            if open_workspaces
                .insert(path.clone(), workspace_id.to_owned())
                .is_some()
            {
                bail!(
                    "herdr worktree list returned a duplicate open worktree path: {}",
                    path.display()
                );
            }
        }

        Ok(Some(Self {
            workspace_id: source_id.to_owned(),
            open_workspaces,
        }))
    }

    pub fn open_workspace_id(&self, path: &Path) -> Option<&str> {
        self.open_workspaces.get(path).map(String::as_str)
    }

    pub fn create(&self, repo_root: &Path, branch: &str, base: &str, target: &Path) -> Result<()> {
        self.run(
            repo_root,
            &[
                "worktree",
                "create",
                "--workspace",
                &self.workspace_id,
                "--branch",
                branch,
                "--base",
                base,
                "--path",
                target.to_str().context("worktree path is not UTF-8")?,
                "--focus",
            ],
        )
    }

    pub fn open(&self, repo_root: &Path, target: &Path) -> Result<()> {
        self.run(
            repo_root,
            &[
                "worktree",
                "open",
                "--workspace",
                &self.workspace_id,
                "--path",
                target.to_str().context("worktree path is not UTF-8")?,
                "--focus",
            ],
        )
    }

    pub fn remove(&self, repo_root: &Path, workspace_id: &str) -> Result<()> {
        self.run(
            repo_root,
            &["worktree", "remove", "--workspace", workspace_id],
        )
    }

    fn run(&self, repo_root: &Path, args: &[&str]) -> Result<()> {
        let output = Command::new("herdr")
            .current_dir(repo_root)
            .args(args)
            .output()
            .with_context(|| format!("could not run herdr worktree {}", args[1]))?;
        if !output.status.success() {
            bail!(
                "herdr worktree {} failed: {}",
                args[1],
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}
