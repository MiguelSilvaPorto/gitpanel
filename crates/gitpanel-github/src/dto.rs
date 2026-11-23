use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct UserDto {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RepoDto {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub owner: OwnerDto,
    pub description: Option<String>,
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub fork: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    pub language: Option<String>,
    #[serde(default)]
    pub stargazers_count: u32,
    pub permissions: Option<PermissionsDto>,
    pub html_url: String,
    pub updated_at: DateTime<Utc>,
    pub pushed_at: Option<DateTime<Utc>>,
}

fn default_branch() -> String {
    "main".to_owned()
}

#[derive(Debug, Deserialize)]
pub struct OwnerDto {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct PermissionsDto {
    #[serde(default)]
    pub admin: bool,
}

#[derive(Debug, Deserialize)]
pub struct BranchDto {
    pub name: String,
    pub commit: BranchCommitDto,
    #[serde(default)]
    pub protected: bool,
}

#[derive(Debug, Deserialize)]
pub struct BranchCommitDto {
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct FileDto {
    pub filename: String,
    pub previous_filename: Option<String>,
    #[serde(default = "modified")]
    pub status: String,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
    #[serde(default)]
    pub changes: u32,
    pub patch: Option<String>,
}

fn modified() -> String {
    "modified".to_owned()
}

#[derive(Debug, Deserialize)]
pub struct CompareDto {
    #[serde(default)]
    pub files: Vec<FileDto>,
    #[serde(default)]
    pub total_commits: u32,
    #[serde(default)]
    pub ahead_by: u32,
    #[serde(default)]
    pub behind_by: u32,
}

#[derive(Debug, Deserialize)]
pub struct CommitDetailDto {
    #[serde(default)]
    pub files: Vec<FileDto>,
}
