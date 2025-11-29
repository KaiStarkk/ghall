use anyhow::Result;
use std::path::Path;
use tokio::process::Command;

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
    pub fn status_icon(&self) -> &'static str {
        if !self.has_remote {
            "?"  // No remote
        } else if self.dirty || self.staged > 0 || self.untracked > 0 {
            "*"  // Dirty
        } else if self.ahead > 0 && self.behind > 0 {
            "⇅"  // Diverged
        } else if self.ahead > 0 {
            "↑"  // Ahead
        } else if self.behind > 0 {
            "↓"  // Behind
        } else {
            "✓"  // Synced
        }
    }

    pub fn status_text(&self) -> String {
        let mut parts = Vec::new();

        if !self.has_remote {
            return "no remote".to_string();
        }

        if self.ahead > 0 && self.behind > 0 {
            parts.push(format!("+{}/-{}", self.ahead, self.behind));
        } else if self.ahead > 0 {
            parts.push(format!("+{} ahead", self.ahead));
        } else if self.behind > 0 {
            parts.push(format!("-{} behind", self.behind));
        }

        if self.dirty {
            parts.push("dirty".to_string());
        }
        if self.staged > 0 {
            parts.push(format!("{} staged", self.staged));
        }
        if self.untracked > 0 {
            parts.push(format!("{} untracked", self.untracked));
        }

        if parts.is_empty() {
            "synced".to_string()
        } else {
            parts.join(", ")
        }
    }

    pub fn is_synced(&self) -> bool {
        self.has_remote
            && self.ahead == 0
            && self.behind == 0
            && !self.dirty
            && self.staged == 0
            && self.untracked == 0
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty || self.staged > 0 || self.untracked > 0
    }

    pub fn can_fast_forward(&self) -> bool {
        self.behind > 0 && self.ahead == 0 && !self.dirty && self.staged == 0
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

    // Check if remote exists
    let remote_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .current_dir(path)
        .output()
        .await?;
    let has_remote = remote_output.status.success();

    let mut status = RepoStatus {
        branch: if branch.is_empty() {
            "HEAD".to_string()
        } else {
            branch
        },
        has_remote,
        ..Default::default()
    };

    if has_remote {
        // Get ahead/behind counts
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

pub async fn fetch(path: &str) -> Result<()> {
    Command::new("git")
        .args(["fetch", "--all", "--prune"])
        .current_dir(path)
        .output()
        .await?;
    Ok(())
}

pub async fn pull(path: &str) -> Result<()> {
    Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(path)
        .output()
        .await?;
    Ok(())
}

pub async fn push(path: &str) -> Result<()> {
    Command::new("git")
        .args(["push"])
        .current_dir(path)
        .output()
        .await?;
    Ok(())
}

pub async fn add_all(path: &str) -> Result<()> {
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output()
        .await?;
    Ok(())
}

pub async fn commit(path: &str, message: &str) -> Result<()> {
    Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(path)
        .output()
        .await?;
    Ok(())
}

pub async fn clone(url: &str, path: &str) -> Result<()> {
    // Create parent directory if needed
    if let Some(parent) = Path::new(path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    Command::new("git")
        .args(["clone", url, path])
        .output()
        .await?;
    Ok(())
}

pub async fn get_dirty_files(path: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .await?;

    let text = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = text
        .lines()
        .map(|line| {
            let status = &line[0..2];
            let file = &line[3..];
            format!("{} {}", status, file)
        })
        .collect();

    Ok(files)
}

pub async fn get_diff(path: &str) -> Result<String> {
    // Get both staged and unstaged changes
    let unstaged = Command::new("git")
        .args(["diff", "--color=never"])
        .current_dir(path)
        .output()
        .await?;

    let staged = Command::new("git")
        .args(["diff", "--cached", "--color=never"])
        .current_dir(path)
        .output()
        .await?;

    let mut result = String::new();

    let staged_text = String::from_utf8_lossy(&staged.stdout);
    if !staged_text.is_empty() {
        result.push_str("=== STAGED CHANGES ===\n");
        result.push_str(&staged_text);
        result.push('\n');
    }

    let unstaged_text = String::from_utf8_lossy(&unstaged.stdout);
    if !unstaged_text.is_empty() {
        result.push_str("=== UNSTAGED CHANGES ===\n");
        result.push_str(&unstaged_text);
    }

    Ok(result)
}

pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}
