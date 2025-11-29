use crate::git::{self, RepoStatus};
use anyhow::Result;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct LocalRepo {
    pub name: String,
    pub path: String,
    pub status: RepoStatus,
    pub remote_owner: Option<String>,
    pub remote_url: Option<String>,
}

pub async fn discover_repos(root: &str) -> Result<Vec<LocalRepo>> {
    let mut repos = Vec::new();

    // Walk directory looking for .git folders
    for entry in WalkDir::new(root)
        .min_depth(1)
        .max_depth(5) // Support deep ghq-style paths
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip hidden dirs except .git, skip common non-repo dirs
            !name.starts_with('.') || name == ".git"
        })
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.file_name() == ".git" && entry.file_type().is_dir() {
            let repo_path = entry.path().parent().unwrap();
            let repo_name = repo_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string());

            let path_str = repo_path.to_string_lossy().to_string();
            let status = git::get_repo_status(&path_str).await.unwrap_or_default();

            // Get remote URL and owner
            let remote_url = git::get_remote_url(&path_str).await;
            let remote_owner = remote_url.as_ref().and_then(|url| parse_owner_from_url(url));

            repos.push(LocalRepo {
                name: repo_name,
                path: path_str,
                status,
                remote_owner,
                remote_url,
            });
        }
    }

    repos.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(repos)
}

fn parse_owner_from_url(url: &str) -> Option<String> {
    // Handle SSH URLs: git@github.com:owner/repo.git
    if url.starts_with("git@") {
        let parts: Vec<&str> = url.split(':').collect();
        if parts.len() == 2 {
            let path = parts[1].trim_end_matches(".git");
            let segments: Vec<&str> = path.split('/').collect();
            if !segments.is_empty() {
                return Some(segments[0].to_string());
            }
        }
    }

    // Handle HTTPS URLs: https://github.com/owner/repo.git
    if url.starts_with("http") {
        let trimmed = url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        let segments: Vec<&str> = trimmed.split('/').collect();
        if segments.len() >= 2 {
            return Some(segments[1].to_string());
        }
    }

    None
}
