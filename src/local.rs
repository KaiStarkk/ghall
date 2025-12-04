use crate::git::{self, RepoStatus};
use anyhow::Result;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct LocalRepo {
    pub name: String,
    pub path: String,
    pub status: RepoStatus,
    pub remote_owner: Option<String>,
    pub remote_url: Option<String>,
    pub last_commit_time: Option<i64>,
    pub is_subrepo: bool,         // Nested inside another repo
    pub parent_repo: Option<String>, // Path to parent repo if subrepo
    pub has_git: bool,            // Whether this folder has a git repo
}

pub async fn discover_repos(root: &str) -> Result<Vec<LocalRepo>> {
    let mut repos = Vec::new();

    // Walk directory looking for .git folders
    // Use follow_links to handle symlinked repos
    for entry in WalkDir::new(root)
        .follow_links(true)
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

        // Check if this is a .git directory (follow symlinks)
        let is_git_dir = entry.file_name() == ".git" && {
            let path = entry.path();
            // Use metadata (follows symlinks) instead of symlink_metadata
            path.is_dir() || Path::new(path).exists()
        };

        if is_git_dir {
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

            // Get last commit time
            let last_commit_time = git::get_last_commit_time(&path_str).await;

            repos.push(LocalRepo {
                name: repo_name,
                path: path_str,
                status,
                remote_owner,
                remote_url,
                last_commit_time,
                is_subrepo: false,
                parent_repo: None,
                has_git: true,
            });
        }
    }

    // Detect subrepos: repos nested inside other repos
    // A repo is a subrepo if its path starts with another repo's path + "/"
    let repo_paths: Vec<String> = repos.iter().map(|r| r.path.clone()).collect();
    for repo in &mut repos {
        for other_path in &repo_paths {
            // Check if this repo's path starts with another repo's path
            // (but is not the same path)
            if repo.path != *other_path && repo.path.starts_with(&format!("{}/", other_path)) {
                repo.is_subrepo = true;
                repo.parent_repo = Some(other_path.clone());
                break; // Found the parent, no need to check more
            }
        }
    }

    // Scan for non-git folders in the "local" subdirectory
    let local_dir = format!("{}/local", root);
    if Path::new(&local_dir).is_dir() {
        if let Ok(entries) = std::fs::read_dir(&local_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        let path = entry.path();
                        let path_str = path.to_string_lossy().to_string();

                        // Check if this directory already has a git repo
                        let git_dir = path.join(".git");
                        let has_git_repo = git_dir.exists();

                        // Skip if we already added it as a git repo
                        if repos.iter().any(|r| r.path == path_str) {
                            continue;
                        }

                        let folder_name = entry.file_name().to_string_lossy().to_string();

                        if has_git_repo {
                            // This is a git repo we missed in the walkdir (shouldn't happen, but be safe)
                            let status = git::get_repo_status(&path_str).await.unwrap_or_default();
                            let remote_url = git::get_remote_url(&path_str).await;
                            let remote_owner = remote_url.as_ref().and_then(|url| parse_owner_from_url(url));
                            let last_commit_time = git::get_last_commit_time(&path_str).await;

                            repos.push(LocalRepo {
                                name: folder_name,
                                path: path_str,
                                status,
                                remote_owner,
                                remote_url,
                                last_commit_time,
                                is_subrepo: false,
                                parent_repo: None,
                                has_git: true,
                            });
                        } else {
                            // Non-git folder - add it with default/empty status
                            repos.push(LocalRepo {
                                name: folder_name,
                                path: path_str,
                                status: RepoStatus::default(),
                                remote_owner: None,
                                remote_url: None,
                                last_commit_time: None,
                                is_subrepo: false,
                                parent_repo: None,
                                has_git: false,
                            });
                        }
                    }
                }
            }
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
