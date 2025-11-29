use crate::app::GistRow;
use crate::git;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct GitHubRepoInfo {
    pub name: String,
    pub owner: String,
    pub url: String,
    pub ssh_url: String,
    pub is_private: bool,
    pub is_fork: bool,
    pub fork_parent: Option<String>,
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
            fork_parent: repo.parent.map(|p| p.name_with_owner),
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
                fork_parent: repo.parent.map(|p| p.name_with_owner),
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

pub async fn create_repo(name: &str, path: &str) -> Result<()> {
    Command::new("gh")
        .args(["repo", "create", name, "--private", "--source", path, "--push"])
        .output()
        .await?;
    Ok(())
}

pub async fn clone_gist(gist_id: &str, path: &str) -> Result<()> {
    Command::new("gh")
        .args(["gist", "clone", gist_id, path])
        .output()
        .await?;
    Ok(())
}

pub async fn delete_gist(gist_id: &str) -> Result<()> {
    Command::new("gh")
        .args(["gist", "delete", gist_id])
        .output()
        .await?;
    Ok(())
}

pub async fn set_visibility(repo: &str, visibility: &str) -> Result<()> {
    Command::new("gh")
        .args(["repo", "edit", repo, "--visibility", visibility])
        .output()
        .await?;
    Ok(())
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
