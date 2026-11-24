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

#[derive(Debug, Deserialize)]
pub struct ReleaseDto {
    pub id: i64,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    pub created_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub html_url: String,
    pub author: Option<ReleaseAuthorDto>,
    #[serde(default)]
    pub assets: Vec<ReleaseAssetDto>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAuthorDto {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAssetDto {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub download_count: u64,
    pub browser_download_url: String,
}

#[derive(Debug, Deserialize)]
pub struct LabelDto {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginDto {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct IssueDto {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub user: Option<LoginDto>,
    #[serde(default)]
    pub labels: Vec<LabelDto>,
    #[serde(default)]
    pub comments: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub html_url: String,
    #[serde(default)]
    pub pull_request: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct PullRefDto {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
}

#[derive(Debug, Deserialize)]
pub struct PullRequestDto {
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    #[serde(default)]
    pub merged: bool,
    #[serde(default)]
    pub draft: bool,
    pub user: Option<LoginDto>,
    pub head: PullRefDto,
    pub base: PullRefDto,
    #[serde(default)]
    pub comments: u32,
    #[serde(default)]
    pub commits: u32,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
    #[serde(default)]
    pub changed_files: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub merged_at: Option<DateTime<Utc>>,
    pub html_url: String,
}

#[derive(Debug, Deserialize)]
pub struct CommentDto {
    pub id: i64,
    pub body: Option<String>,
    pub user: Option<LoginDto>,
    pub created_at: DateTime<Utc>,
    pub html_url: Option<String>,
}
