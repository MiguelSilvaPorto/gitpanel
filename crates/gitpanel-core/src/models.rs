use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A repository returned by the GitHub API and kept in the local cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub github_id: i64,
    pub owner: String,
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub private: bool,
    pub visibility: String,
    pub fork: bool,
    pub archived: bool,
    pub default_branch: String,
    pub language: Option<String>,
    pub stars: u32,
    pub permissions_admin: bool,
    pub html_url: String,
    pub updated_at: DateTime<Utc>,
    pub pushed_at: Option<DateTime<Utc>>,
    pub synced_at: Option<DateTime<Utc>>,
    pub monitored: bool,
    pub last_sync_error: Option<String>,
}

impl Repository {
    pub fn is_private(&self) -> bool {
        self.private
    }

    pub fn is_archived(&self) -> bool {
        self.archived
    }

    pub fn display_owner(&self) -> &str {
        &self.owner
    }

    pub fn short_name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub scopes: Vec<String>,
}

impl Account {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.login)
    }

    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|item| item == scope)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    pub sha: String,
    pub protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentEntry {
    pub name: String,
    pub path: String,
    pub sha: String,
    pub kind: String,
    pub size: Option<u64>,
    pub html_url: Option<String>,
    pub download_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContentPayload {
    pub path: String,
    pub kind: String,
    pub sha: Option<String>,
    pub size: Option<u64>,
    pub content: Option<String>,
    pub entries: Vec<ContentEntry>,
    pub html_url: Option<String>,
    pub truncated: bool,
    pub binary: bool,
}

impl ContentPayload {
    pub fn is_dir(&self) -> bool {
        self.kind == "dir"
    }

    pub fn is_file(&self) -> bool {
        self.kind == "file"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
    pub download_count: u64,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Release {
    pub id: i64,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub html_url: String,
    pub author: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub user: Option<String>,
    pub labels: Vec<Label>,
    pub comments: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub merged: bool,
    pub draft: bool,
    pub user: Option<String>,
    pub head_ref: String,
    pub head_sha: String,
    pub base_ref: String,
    pub base_sha: String,
    pub comments: u32,
    pub commits: u32,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
    pub html_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueComment {
    pub id: i64,
    pub body: Option<String>,
    pub user: Option<String>,
    pub created_at: DateTime<Utc>,
    pub html_url: Option<String>,
}

/// Commit data as delivered by GitHub, before it is associated with a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitData {
    pub sha: String,
    pub author_login: Option<String>,
    pub author_name: String,
    pub author_email: Option<String>,
    pub committed_at: DateTime<Utc>,
    pub message: String,
    pub additions: u32,
    pub deletions: u32,
    pub is_merge: bool,
}

/// A commit stored in the local database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub repo_id: i64,
    pub sha: String,
    pub author_login: Option<String>,
    pub author_name: String,
    pub author_email: Option<String>,
    pub committed_at: DateTime<Utc>,
    pub message: String,
    pub additions: u32,
    pub deletions: u32,
    pub branch: String,
    pub is_merge: bool,
}

impl Commit {
    pub fn short_sha(&self) -> &str {
        self.sha.get(..7).unwrap_or(&self.sha)
    }

    pub fn author_display(&self) -> &str {
        self.author_login.as_deref().unwrap_or(&self.author_name)
    }
}

impl From<(i64, &str, CommitData)> for Commit {
    fn from((repo_id, branch, data): (i64, &str, CommitData)) -> Self {
        Self {
            repo_id,
            sha: data.sha,
            author_login: data.author_login,
            author_name: data.author_name,
            author_email: data.author_email,
            committed_at: data.committed_at,
            message: data.message,
            additions: data.additions,
            deletions: data.deletions,
            branch: branch.to_owned(),
            is_merge: data.is_merge,
        }
    }
}

/// A file patch returned by the compare/commit endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePatch {
    pub filename: String,
    pub previous_filename: Option<String>,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub changes: u32,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub files: Vec<FilePatch>,
    pub total_commits: u32,
    pub ahead_by: u32,
    pub behind_by: u32,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitDiffResult {
    pub sha: String,
    pub files: Vec<FilePatch>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub limit: u32,
    pub remaining: u32,
    pub reset_at: DateTime<Utc>,
    pub cost: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateRepoRequest {
    pub name: String,
    pub description: Option<String>,
    pub private: bool,
    pub auto_init: bool,
    pub gitignore_template: Option<String>,
    pub license_template: Option<String>,
}
