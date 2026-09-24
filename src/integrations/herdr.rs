use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub struct Herdr {
    workspace_id: String,
}

impl Herdr {
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
        let source_id = source_workspace_id(&list).unwrap_or(&workspace_id);
        Ok(Some(Self {
            workspace_id: source_id.to_owned(),
        }))
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

fn source_workspace_id(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => map
            .get("source_workspace_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .or_else(|| map.values().find_map(source_workspace_id)),
        Value::Array(items) => items.iter().find_map(source_workspace_id),
        _ => None,
    }
}
