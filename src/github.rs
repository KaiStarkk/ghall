use crate::app::GistRow;
use crate::git;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tokio::process::Command;

/// SSH command that auto-accepts new host keys (but rejects changed ones for security)
const SSH_COMMAND: &str = "ssh -o StrictHostKeyChecking=accept-new -o BatchMode=yes";

/// Result of a GitHub CLI operation with captured output
#[derive(Debug, Clone)]
pub struct GhOpResult {
    pub success: bool,
    pub stderr: String,
}

impl GhOpResult {
    pub fn ok() -> Self {
        Self { success: true, stderr: String::new() }
    }

    pub fn err(stderr: String) -> Self {
        Self { success: false, stderr }
    }
}

#[derive(Debug, Clone)]
pub struct GitHubRepoInfo {
    pub name: String,
    pub owner: String,
    pub url: String,
    pub ssh_url: String,
    pub is_private: bool,
    pub is_fork: bool,
    pub is_archived: bool,
    pub fork_parent: Option<String>,
    pub is_member: bool, // User owns or is member of org
}

// GraphQL response types
#[derive(Debug, Deserialize)]
struct GraphQLResponse {
    data: Option<GraphQLData>,
}

#[derive(Debug, Deserialize)]
struct GraphQLData {
    viewer: Viewer,
}

#[derive(Debug, Deserialize)]
struct Viewer {
    login: String,
    repositories: RepositoryConnection,
    organizations: OrganizationConnection,
}

#[derive(Debug, Deserialize)]
struct RepositoryConnection {
    nodes: Vec<Repository>,
}

#[derive(Debug, Deserialize)]
struct Repository {
    name: String,
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
    url: String,
    #[serde(rename = "sshUrl")]
    ssh_url: String,
    #[serde(rename = "isPrivate")]
    is_private: bool,
    #[serde(rename = "isFork")]
    is_fork: bool,
    #[serde(rename = "isArchived")]
    is_archived: bool,
    parent: Option<ParentRepo>,
}

#[derive(Debug, Deserialize)]
struct ParentRepo {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

#[derive(Debug, Deserialize)]
struct OrganizationConnection {
    nodes: Vec<Organization>,
}

#[derive(Debug, Deserialize)]
struct Organization {
    login: String,
    repositories: RepositoryConnection,
}

// Gist types
#[derive(Debug, Deserialize)]
pub struct GitHubGist {
    pub id: String,
    pub description: Option<String>,
    pub public: bool,
    #[serde(rename = "html_url")]
    pub html_url: String,
    pub files: HashMap<String, GistFile>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GistFile {
    pub filename: String,
}

const GRAPHQL_QUERY: &str = r#"
query {
  viewer {
    login
    repositories(first: 100, ownerAffiliations: OWNER) {
      nodes {
        name
        nameWithOwner
        url
        sshUrl
        isPrivate
        isFork
        isArchived
        parent { nameWithOwner }
      }
    }
    organizations(first: 50) {
      nodes {
        login
        repositories(first: 100) {
          nodes {
            name
            nameWithOwner
            url
            sshUrl
            isPrivate
            isFork
            isArchived
            parent { nameWithOwner }
          }
        }
      }
    }
  }
}
"#;

pub async fn fetch_all_repos_graphql() -> Result<Vec<GitHubRepoInfo>> {
    let output = Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={}", GRAPHQL_QUERY)])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("GraphQL query failed: {}", stderr);
    }

    let response: GraphQLResponse = serde_json::from_slice(&output.stdout)?;

    let data = response.data.ok_or_else(|| anyhow::anyhow!("No data in GraphQL response"))?;

    let mut repos = Vec::new();

    // Add user's own repos
    for repo in data.viewer.repositories.nodes {
        let owner = repo.name_with_owner
            .split('/')
            .next()
            .unwrap_or(&data.viewer.login)
            .to_string();

        repos.push(GitHubRepoInfo {
            name: repo.name,
            owner,
            url: repo.url,
            ssh_url: repo.ssh_url,
            is_private: repo.is_private,
            is_fork: repo.is_fork,
            is_archived: repo.is_archived,
            fork_parent: repo.parent.map(|p| p.name_with_owner),
            is_member: true, // User's own repos
        });
    }

    // Add org repos
    for org in data.viewer.organizations.nodes {
        for repo in org.repositories.nodes {
            repos.push(GitHubRepoInfo {
                name: repo.name,
                owner: org.login.clone(),
                url: repo.url,
                ssh_url: repo.ssh_url,
                is_private: repo.is_private,
                is_fork: repo.is_fork,
                is_archived: repo.is_archived,
                fork_parent: repo.parent.map(|p| p.name_with_owner),
                is_member: true, // User is member of org
            });
        }
    }

    Ok(repos)
}

pub async fn fetch_gists_as_rows(local_root: &str) -> Result<Vec<GistRow>> {
    let output = Command::new("gh")
        .args(["api", "gists", "--paginate"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let gists: Vec<GitHubGist> = serde_json::from_slice(&output.stdout).unwrap_or_default();

    let gists_dir = format!("{}/gists", local_root);
    let mut rows = Vec::new();

    for g in gists {
        let potential_path = format!("{}/{}", gists_dir, g.id);
        // Use symlink-following exists check
        let local_path = if Path::new(&potential_path).exists() {
            Some(potential_path.clone())
        } else {
            None
        };

        // Get git status if local
        let git_status = if local_path.is_some() {
            git::get_repo_status(&potential_path).await.ok()
        } else {
            None
        };

        let description = g.description.unwrap_or_else(|| {
            g.files.keys().next().cloned().unwrap_or_else(|| "Untitled".to_string())
        });

        let file_names: Vec<String> = g.files.values().map(|f| f.filename.clone()).collect();

        rows.push(GistRow {
            id: g.id,
            description,
            is_public: g.public,
            file_names,
            html_url: g.html_url,
            local_path,
            git_status,
            created_at: g.created_at,
            updated_at: g.updated_at,
        });
    }

    Ok(rows)
}

/// Options for creating a new repository
pub struct CreateRepoOptions {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub private: bool,
    pub org: Option<String>, // None = personal account
}

pub async fn create_repo(opts: &CreateRepoOptions) -> GhOpResult {
    let mut args = vec!["repo", "create"];

    // Build full name (org/name or just name for personal)
    let full_name = if let Some(ref org) = opts.org {
        format!("{}/{}", org, opts.name)
    } else {
        opts.name.clone()
    };
    args.push(&full_name);

    if opts.private {
        args.push("--private");
    } else {
        args.push("--public");
    }

    args.push("--source");
    args.push(&opts.path);
    args.push("--push");

    // Add description if provided
    let desc_arg;
    if let Some(ref desc) = opts.description {
        if !desc.is_empty() {
            args.push("--description");
            desc_arg = desc.clone();
            args.push(&desc_arg);
        }
    }

    let output = Command::new("gh")
        .args(&args)
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GhOpResult::ok(),
        Ok(out) => GhOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GhOpResult::err(e.to_string()),
    }
}

pub async fn get_user_orgs() -> Result<Vec<String>> {
    let output = Command::new("gh")
        .args(["api", "user/orgs", "--jq", ".[].login"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let orgs: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect();

    Ok(orgs)
}

pub async fn clone_gist(gist_id: &str, path: &str) -> GhOpResult {
    let output = Command::new("gh")
        .args(["gist", "clone", gist_id, path])
        .env("GIT_SSH_COMMAND", SSH_COMMAND)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GhOpResult::ok(),
        Ok(out) => GhOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GhOpResult::err(e.to_string()),
    }
}

pub async fn delete_gist(gist_id: &str) -> GhOpResult {
    let output = Command::new("gh")
        .args(["gist", "delete", gist_id])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GhOpResult::ok(),
        Ok(out) => GhOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GhOpResult::err(e.to_string()),
    }
}

pub async fn delete_repo(repo: &str) -> GhOpResult {
    let output = Command::new("gh")
        .args(["repo", "delete", repo, "--yes"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GhOpResult::ok(),
        Ok(out) => GhOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GhOpResult::err(e.to_string()),
    }
}

pub async fn set_visibility(repo: &str, visibility: &str) -> GhOpResult {
    let output = Command::new("gh")
        .args(["repo", "edit", repo, "--visibility", visibility, "--accept-visibility-change-consequences"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => GhOpResult::ok(),
        Ok(out) => GhOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => GhOpResult::err(e.to_string()),
    }
}

pub async fn set_archived(repo: &str, archived: bool) -> GhOpResult {
    if archived {
        let output = Command::new("gh")
            .args(["repo", "archive", repo, "--yes"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => GhOpResult::ok(),
            Ok(out) => GhOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
            Err(e) => GhOpResult::err(e.to_string()),
        }
    } else {
        // Use API to unarchive (gh repo archive doesn't support --unarchive)
        let output = Command::new("gh")
            .args(["api", "-X", "PATCH", &format!("/repos/{}", repo), "-f", "archived=false"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => GhOpResult::ok(),
            Ok(out) => GhOpResult::err(String::from_utf8_lossy(&out.stderr).to_string()),
            Err(e) => GhOpResult::err(e.to_string()),
        }
    }
}

pub async fn get_current_user() -> Result<String> {
    let output = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!("Failed to get current user");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
