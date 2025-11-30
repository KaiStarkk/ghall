use anyhow::Result;
use std::path::Path;
use tokio::process::Command;

/// Result of a git operation with captured output
#[derive(Debug, Clone)]
pub struct GitOpResult {
    pub success: bool,
    pub stderr: String,
}

impl GitOpResult {
    pub fn ok() -> Self {
        Self { success: true, stderr: String::new() }
    }

    pub fn err(stderr: String) -> Self {
        Self { success: false, stderr }
    }
}

/// SSH command that auto-accepts new host keys (but rejects changed ones for security)
const SSH_COMMAND: &str = "ssh -o StrictHostKeyChecking=accept-new -o BatchMode=yes";

#[derive(Debug, Clone, Default)]
pub struct RepoStatus {
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
    pub untracked: u32,
    pub staged: u32,
    pub has_remote: bool,
}

impl RepoStatus {
    pub fn is_dirty(&self) -> bool {
        self.dirty || self.staged > 0 || self.untracked > 0
    }
}

pub async fn get_repo_status(path: &str) -> Result<RepoStatus> {
    let path = Path::new(path);

    // Get current branch
    let branch_output = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(path)
        .output()
        .await?;
    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // Check if there are any remotes
    let remotes_output = Command::new("git")
        .args(["remote"])
        .current_dir(path)
        .output()
        .await?;
    let has_any_remote = remotes_output.status.success()
        && !String::from_utf8_lossy(&remotes_output.stdout).trim().is_empty();

    // Check if upstream tracking branch exists
    let upstream_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .current_dir(path)
        .output()
        .await?;
    let has_upstream = upstream_output.status.success();

    let branch_name = if branch.is_empty() {
        "HEAD".to_string()
    } else {
        branch.clone()
    };

    let mut status = RepoStatus {
        branch: branch_name.clone(),
        has_remote: has_any_remote,
        ..Default::default()
    };

    // Try to get ahead/behind counts
    if has_upstream {
        // Use configured upstream
        let rev_list = Command::new("git")
            .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
            .current_dir(path)
            .output()
            .await?;

        if rev_list.status.success() {
            let counts = String::from_utf8_lossy(&rev_list.stdout);
            let parts: Vec<&str> = counts.trim().split('\t').collect();
            if parts.len() == 2 {
                status.ahead = parts[0].parse().unwrap_or(0);
                status.behind = parts[1].parse().unwrap_or(0);
            }
        }
    } else if has_any_remote && !branch.is_empty() {
        // Fallback: try origin/<branch> if it exists
        let ref_check = Command::new("git")
            .args(["rev-parse", "--verify", &format!("origin/{}", branch)])
            .current_dir(path)
            .output()
            .await?;

        if ref_check.status.success() {
            let rev_list = Command::new("git")
                .args([
                    "rev-list",
                    "--left-right",
                    "--count",
                    &format!("HEAD...origin/{}", branch),
                ])
                .current_dir(path)
                .output()
                .await?;

            if rev_list.status.success() {
                let counts = String::from_utf8_lossy(&rev_list.stdout);
                let parts: Vec<&str> = counts.trim().split('\t').collect();
                if parts.len() == 2 {
                    status.ahead = parts[0].parse().unwrap_or(0);
                    status.behind = parts[1].parse().unwrap_or(0);
                }
            }
        }
    }

    // Get working tree status
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .await?;

    if status_output.status.success() {
        let status_text = String::from_utf8_lossy(&status_output.stdout);
        for line in status_text.lines() {
            if line.len() >= 2 {
                let index = line.chars().next().unwrap_or(' ');
                let worktree = line.chars().nth(1).unwrap_or(' ');

                if index == '?' {
                    status.untracked += 1;
                } else {
                    if index != ' ' {
                        status.staged += 1;
                    }
                    if worktree != ' ' {
                        status.dirty = true;
                    }
                }
            }
        }
    }

    Ok(status)
}

pub async fn get_remote_url(path: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

pub async fn fetch(path: &str) -> GitOpResult {
    let output = Command::new("git")
        .args(["fetch", "--all", "--prune"])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .current_dir(path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GitOpResult::ok(),
        Ok(out) => GitOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

pub async fn pull(path: &str) -> GitOpResult {
    let output = Command::new("git")
        .args(["pull", "--ff-only"])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .current_dir(path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GitOpResult::ok(),
        Ok(out) => GitOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

pub async fn push(path: &str) -> GitOpResult {
    let output = Command::new("git")
        .args(["push"])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .current_dir(path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GitOpResult::ok(),
        Ok(out) => GitOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

pub async fn clone(url: &str, path: &str) -> GitOpResult {
    // Create parent directory if needed
    if let Some(parent) = Path::new(path).parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return GitOpResult::err(format!("Failed to create directory: {}", e));
        }
    }

    let output = Command::new("git")
        .args(["clone", url, path])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GitOpResult::ok(),
        Ok(out) => GitOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GitOpResult::err(e.to_string()),
    }
}

/// Quicksync: fetch, ff-rebase, add all, commit with fixup, push
pub async fn quicksync(path: &str) -> GitOpResult {
    let path = Path::new(path);

    // 1. Fetch
    let fetch = Command::new("git")
        .args(["fetch", "--all", "--prune"])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .current_dir(path)
        .output()
        .await;

    if let Ok(out) = &fetch {
        if !out.status.success() {
            return GitOpResult::err(format!("Fetch failed: {}", String::from_utf8_lossy(&out.stderr)));
        }
    } else if let Err(e) = fetch {
        return GitOpResult::err(format!("Fetch failed: {}", e));
    }

    // 2. Fast-forward rebase (only if there are upstream changes)
    let rebase = Command::new("git")
        .args(["rebase", "--autostash"])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .current_dir(path)
        .output()
        .await;

    if let Ok(out) = &rebase {
        if !out.status.success() {
            // Abort the rebase if it fails
            let _ = Command::new("git")
                .args(["rebase", "--abort"])
                .current_dir(path)
                .output()
                .await;
            return GitOpResult::err(format!("Rebase failed: {}", String::from_utf8_lossy(&out.stderr)));
        }
    } else if let Err(e) = rebase {
        return GitOpResult::err(format!("Rebase failed: {}", e));
    }

    // 3. Add all changes
    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output()
        .await;

    if let Err(e) = add {
        return GitOpResult::err(format!("Add failed: {}", e));
    }

    // 4. Check if there are staged changes
    let status = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(path)
        .output()
        .await;

    let has_staged = status.map(|o| !o.status.success()).unwrap_or(false);

    // 5. Commit with fixup message if there are changes
    if has_staged {
        let commit = Command::new("git")
            .args(["commit", "-m", "fixup"])
            .current_dir(path)
            .output()
            .await;

        if let Ok(out) = &commit {
            if !out.status.success() {
                return GitOpResult::err(format!("Commit failed: {}", String::from_utf8_lossy(&out.stderr)));
            }
        } else if let Err(e) = commit {
            return GitOpResult::err(format!("Commit failed: {}", e));
        }
    }

    // 6. Push
    let push = Command::new("git")
        .args(["push"])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .current_dir(path)
        .output()
        .await;

    match push {
        Ok(out) if out.status.success() => GitOpResult::ok(),
        Ok(out) => GitOpResult::err(format!("Push failed: {}", String::from_utf8_lossy(&out.stderr))),
        Err(e) => GitOpResult::err(format!("Push failed: {}", e)),
    }
}

/// Get the Unix timestamp of the last commit
pub async fn get_last_commit_time(path: &str) -> Option<i64> {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .current_dir(path)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()
    } else {
        None
    }
}
