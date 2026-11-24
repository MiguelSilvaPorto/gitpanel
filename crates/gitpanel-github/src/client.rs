use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{DateTime, Utc};
use gitpanel_core::{
    Account, Branch, CommitData, CommitDiffResult, CompareResult, ContentEntry, ContentPayload,
    CreateRepoRequest, FilePatch, Issue, IssueComment, Label, PullRequest, RateLimit, Release,
    ReleaseAsset, RepoTree, RepoTreeEntry, Repository, WorkflowJob, WorkflowRun, WorkflowStep,
};
use reqwest::header::{ACCEPT, HeaderMap};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use serde_json::{Value, json};

use crate::auth::Token;
use crate::dto::{
    BranchDto, CommentDto, CommitDetailDto, CompareDto, FileDto, GitTreeDto, IssueDto,
    PullRequestDto, ReleaseDto, RepoDto, UserDto, WorkflowJobDto, WorkflowJobsDto, WorkflowRunDto,
    WorkflowRunsDto,
};
use crate::error::GithubError;

const API_BASE: &str = "https://api.github.com";
const PER_PAGE: usize = 100;
const COMPARE_FILE_LIMIT: usize = 300;

#[derive(Debug, Clone)]
pub struct CommitPage {
    pub commits: Vec<CommitData>,
    pub has_next: bool,
    pub end_cursor: Option<String>,
    pub rate_limit: Option<RateLimit>,
}

#[derive(Debug, Clone, Copy)]
pub struct FileUpdate<'a> {
    pub owner: &'a str,
    pub repo: &'a str,
    pub path: &'a str,
    pub branch: &'a str,
    pub content: &'a str,
    pub message: &'a str,
    pub sha: &'a str,
}

#[derive(Clone)]
pub struct GithubClient {
    http: Client,
    token: Arc<Token>,
    api_base: String,
}

impl GithubClient {
    pub fn from_gh() -> Result<Self, GithubError> {
        Self::new(Token::from_gh()?)
    }

    pub fn new(token: Token) -> Result<Self, GithubError> {
        Self::with_base(token, API_BASE)
    }

    pub fn with_base(token: Token, api_base: impl Into<String>) -> Result<Self, GithubError> {
        let http = Client::builder()
            .user_agent("gitpanel/0.1.0")
            .timeout(Duration::from_secs(30))
            .gzip(true)
            .build()?;
        Ok(Self {
            http,
            token: Arc::new(token),
            api_base: api_base.into().trim_end_matches('/').to_owned(),
        })
    }

    pub async fn get_account(&self) -> Result<Account, GithubError> {
        let request = self.http.get(format!("{}/user", self.api_base));
        let (_, headers, body) = self.send(request).await?;
        let user: UserDto = decode(&body, "user")?;
        let scopes = headers
            .get("x-oauth-scopes")
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|scope| !scope.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Account {
            login: user.login,
            name: user.name,
            avatar_url: user.avatar_url,
            scopes,
        })
    }

    pub async fn list_repositories(&self) -> Result<Vec<Repository>, GithubError> {
        let mut repositories = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let request = self
                .http
                .get(format!("{}/user/repos", self.api_base))
                .query(&[
                    ("affiliation", "owner,collaborator,organization_member"),
                    ("visibility", "all"),
                    ("sort", "updated"),
                    ("direction", "desc"),
                    ("per_page", "100"),
                    ("page", page_value.as_str()),
                ]);
            let (_, _, body) = self.send(request).await?;
            let page_items: Vec<RepoDto> = decode(&body, "repository list")?;
            let count = page_items.len();
            repositories.extend(page_items.into_iter().map(repository_from_dto));
            if count < PER_PAGE || page >= 1000 {
                break;
            }
            page += 1;
        }
        Ok(repositories)
    }

    pub async fn create_repository(
        &self,
        request: &CreateRepoRequest,
    ) -> Result<Repository, GithubError> {
        let mut body = serde_json::Map::new();
        body.insert("name".to_owned(), json!(&request.name));
        body.insert("private".to_owned(), json!(request.private));
        body.insert("auto_init".to_owned(), json!(request.auto_init));
        if let Some(description) = &request.description
            && !description.trim().is_empty()
        {
            body.insert("description".to_owned(), json!(description));
        }
        if let Some(template) = &request.gitignore_template
            && !template.is_empty()
        {
            body.insert("gitignore_template".to_owned(), json!(template));
        }
        if let Some(template) = &request.license_template
            && !template.is_empty()
        {
            body.insert("license_template".to_owned(), json!(template));
        }
        let http_request = self
            .http
            .post(format!("{}/user/repos", self.api_base))
            .json(&Value::Object(body));
        let (_, _, response) = self.send(http_request).await?;
        let dto: RepoDto = decode(&response, "created repository")?;
        Ok(repository_from_dto(dto))
    }

    pub async fn list_branches(&self, owner: &str, repo: &str) -> Result<Vec<Branch>, GithubError> {
        let mut branches = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let request = self
                .http
                .get(format!("{}/repos/{owner}/{repo}/branches", self.api_base))
                .query(&[("per_page", "100"), ("page", page_value.as_str())]);
            let (_, _, body) = self.send(request).await?;
            let page_items: Vec<BranchDto> = decode(&body, "branch list")?;
            let count = page_items.len();
            branches.extend(page_items.into_iter().map(|branch| Branch {
                name: branch.name,
                sha: branch.commit.sha,
                protected: branch.protected,
            }));
            if count < PER_PAGE || page >= 1000 {
                break;
            }
            page += 1;
        }
        Ok(branches)
    }

    pub async fn compare(
        &self,
        owner: &str,
        repo: &str,
        base_sha: &str,
        head_sha: &str,
    ) -> Result<CompareResult, GithubError> {
        let request = self.http.get(format!(
            "{}/repos/{owner}/{repo}/compare/{base_sha}...{head_sha}",
            self.api_base
        ));
        let (_, _, body) = self.send(request).await?;
        let dto: CompareDto = decode(&body, "compare")?;
        let truncated = dto.files.len() >= COMPARE_FILE_LIMIT;
        Ok(CompareResult {
            files: dto.files.into_iter().map(file_from_dto).collect(),
            total_commits: dto.total_commits,
            ahead_by: dto.ahead_by,
            behind_by: dto.behind_by,
            truncated,
        })
    }

    pub async fn commit_diff(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<CommitDiffResult, GithubError> {
        let request = self.http.get(format!(
            "{}/repos/{owner}/{repo}/commits/{sha}",
            self.api_base
        ));
        let (_, _, body) = self.send(request).await?;
        let dto: CommitDetailDto = decode(&body, "commit diff")?;
        let truncated = dto.files.len() >= COMPARE_FILE_LIMIT;
        Ok(CommitDiffResult {
            sha: sha.to_owned(),
            files: dto.files.into_iter().map(file_from_dto).collect(),
            truncated,
        })
    }

    pub async fn fetch_commits_page(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        since: DateTime<Utc>,
        cursor: Option<&str>,
    ) -> Result<CommitPage, GithubError> {
        let body = json!({
            "query": COMMIT_QUERY,
            "variables": {
                "owner": owner,
                "name": repo,
                "branch": format!("refs/heads/{branch}"),
                "since": since.to_rfc3339(),
                "cursor": cursor,
            }
        });
        let request = self
            .http
            .post(format!("{}/graphql", self.api_base))
            .json(&body);
        let (_, _, response) = self.send(request).await?;
        let root: Value = serde_json::from_str(&response).map_err(|error| GithubError::Decode {
            context: "GraphQL commits".to_owned(),
            details: error.to_string(),
        })?;

        if root["errors"].is_array() && root["data"].is_null() {
            let message = root["errors"]
                .as_array()
                .and_then(|errors| errors.first())
                .and_then(|error| error["message"].as_str())
                .unwrap_or("unknown GraphQL error")
                .to_owned();
            return Err(GithubError::GraphQl { message });
        }

        let rate_limit = parse_rate_limit(&root["data"]["rateLimit"]);
        let repository = &root["data"]["repository"];
        if repository.is_null() {
            return Err(GithubError::NotFound {
                message: format!("{owner}/{repo}"),
            });
        }

        let history = &repository["ref"]["target"]["history"];
        if history.is_null() {
            return Ok(CommitPage {
                commits: Vec::new(),
                has_next: false,
                end_cursor: None,
                rate_limit,
            });
        }

        let mut commits = Vec::new();
        if let Some(nodes) = history["nodes"].as_array() {
            for node in nodes {
                let Some(sha) = node["oid"].as_str() else {
                    continue;
                };
                let Some(committed_at) = node["committedDate"]
                    .as_str()
                    .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.with_timezone(&Utc))
                else {
                    continue;
                };
                let author = &node["author"];
                let committer = &node["committer"];
                let author_login = author["user"]["login"]
                    .as_str()
                    .or_else(|| committer["user"]["login"].as_str())
                    .map(str::to_owned);
                let author_name = author["name"]
                    .as_str()
                    .or_else(|| committer["name"].as_str())
                    .unwrap_or("Desconhecido")
                    .to_owned();
                let author_email = author["email"]
                    .as_str()
                    .or_else(|| committer["email"].as_str())
                    .map(str::to_owned);
                commits.push(CommitData {
                    sha: sha.to_owned(),
                    author_login,
                    author_name,
                    author_email,
                    committed_at,
                    message: node["messageHeadline"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    additions: node["additions"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
                    deletions: node["deletions"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
                    is_merge: node["parents"]["totalCount"].as_u64().unwrap_or(0) > 1,
                });
            }
        }

        Ok(CommitPage {
            commits,
            has_next: history["pageInfo"]["hasNextPage"]
                .as_bool()
                .unwrap_or(false),
            end_cursor: history["pageInfo"]["endCursor"].as_str().map(str::to_owned),
            rate_limit,
        })
    }

    pub async fn get_contents(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        reference: &str,
    ) -> Result<ContentPayload, GithubError> {
        let url = self.contents_url(owner, repo, path, reference)?;
        let (_, _, body) = self.send(self.http.get(url)).await?;
        let value: Value = decode(&body, "repository contents")?;
        content_payload_from_value(&value, path)
    }

    pub async fn get_tree(
        &self,
        owner: &str,
        repo: &str,
        reference: &str,
    ) -> Result<RepoTree, GithubError> {
        let mut url = Url::parse(&format!("{}/repos/{owner}/{repo}/git/trees", self.api_base))
            .map_err(|error| GithubError::Decode {
                context: "tree URL".to_owned(),
                details: error.to_string(),
            })?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| GithubError::Decode {
                context: "tree URL".to_owned(),
                details: "URL cannot be a base URL".to_owned(),
            })?;
            segments.push(reference);
        }
        url.query_pairs_mut().append_pair("recursive", "1");
        let (_, _, body) = self.send(self.http.get(url)).await?;
        let dto: GitTreeDto = decode(&body, "repository tree")?;
        let entries = dto
            .tree
            .into_iter()
            .filter_map(|item| {
                let name = item
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&item.path)
                    .to_owned();
                let kind = match item.kind.as_str() {
                    "tree" => "dir",
                    "blob" => "file",
                    "commit" => "file",
                    _ => return None,
                };
                Some(RepoTreeEntry {
                    path: item.path,
                    name,
                    kind: kind.to_owned(),
                    size: item.size,
                    sha: item.sha,
                })
            })
            .collect();
        Ok(RepoTree {
            entries,
            truncated: dto.truncated,
        })
    }

    pub async fn update_file(&self, update: FileUpdate<'_>) -> Result<CommitData, GithubError> {
        let url = self.contents_url(update.owner, update.repo, update.path, "")?;
        let body = json!({
            "message": update.message,
            "content": BASE64.encode(update.content.as_bytes()),
            "branch": update.branch,
            "sha": update.sha,
        });
        let (_, _, response) = self.send(self.http.put(url).json(&body)).await?;
        let value: Value = decode(&response, "file update")?;
        let commit = &value["commit"];
        let commit_sha = commit["sha"].as_str().ok_or_else(|| GithubError::Decode {
            context: "file update".to_owned(),
            details: "missing commit sha".to_owned(),
        })?;
        let commit_message = commit["message"]
            .as_str()
            .unwrap_or(update.message)
            .to_owned();
        let author = &commit["author"];
        let committed_at = author["date"]
            .as_str()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        Ok(CommitData {
            sha: commit_sha.to_owned(),
            author_login: None,
            author_name: author["name"].as_str().unwrap_or("GitHub").to_owned(),
            author_email: author["email"].as_str().map(str::to_owned),
            committed_at,
            message: commit_message,
            additions: 1,
            deletions: 1,
            is_merge: false,
        })
    }

    pub async fn get_readme(
        &self,
        owner: &str,
        repo: &str,
        reference: &str,
    ) -> Result<Option<ContentPayload>, GithubError> {
        let mut url = Url::parse(&format!("{}/repos/{owner}/{repo}/readme", self.api_base))
            .map_err(|error| GithubError::Decode {
                context: "readme URL".to_owned(),
                details: error.to_string(),
            })?;
        if !reference.is_empty() {
            url.query_pairs_mut().append_pair("ref", reference);
        }
        let (_, _, body) = match self.send(self.http.get(url)).await {
            Ok(response) => response,
            Err(GithubError::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        let value: Value = decode(&body, "readme")?;
        Ok(Some(content_payload_from_value(&value, "")?))
    }

    pub async fn list_releases(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<Release>, GithubError> {
        let mut releases = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let url = Url::parse(&format!("{}/repos/{owner}/{repo}/releases", self.api_base))
                .map_err(|error| GithubError::Decode {
                    context: "releases URL".to_owned(),
                    details: error.to_string(),
                })?;
            let request = self
                .http
                .get(url)
                .query(&[("per_page", "100"), ("page", page_value.as_str())]);
            let (_, _, body) = self.send(request).await?;
            let page_items: Vec<ReleaseDto> = decode(&body, "releases")?;
            let count = page_items.len();
            releases.extend(page_items.into_iter().map(release_from_dto));
            if count < PER_PAGE || page >= 100 {
                break;
            }
            page += 1;
        }
        Ok(releases)
    }

    pub async fn list_issues(
        &self,
        owner: &str,
        repo: &str,
        state: &str,
    ) -> Result<Vec<Issue>, GithubError> {
        let mut issues = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let url = Url::parse(&format!("{}/repos/{owner}/{repo}/issues", self.api_base))
                .map_err(|error| GithubError::Decode {
                    context: "issues URL".to_owned(),
                    details: error.to_string(),
                })?;
            let request = self.http.get(url).query(&[
                ("state", state),
                ("sort", "updated"),
                ("direction", "desc"),
                ("per_page", "100"),
                ("page", page_value.as_str()),
            ]);
            let (_, _, body) = self.send(request).await?;
            let page_items: Vec<IssueDto> = decode(&body, "issues")?;
            let count = page_items.len();
            issues.extend(
                page_items
                    .into_iter()
                    .filter(|issue| issue.pull_request.is_none())
                    .map(|issue| issue_from_dto(issue, false)),
            );
            if count < PER_PAGE || page >= 100 {
                break;
            }
            page += 1;
        }
        Ok(issues)
    }

    pub async fn get_issue(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<Issue, GithubError> {
        let url = Url::parse(&format!(
            "{}/repos/{owner}/{repo}/issues/{number}",
            self.api_base
        ))
        .map_err(|error| GithubError::Decode {
            context: "issue URL".to_owned(),
            details: error.to_string(),
        })?;
        let (_, _, body) = self.send(self.http.get(url)).await?;
        let dto: IssueDto = decode(&body, "issue")?;
        Ok(issue_from_dto(dto, true))
    }

    pub async fn list_issue_comments(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<Vec<IssueComment>, GithubError> {
        let mut comments = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let url = Url::parse(&format!(
                "{}/repos/{owner}/{repo}/issues/{number}/comments",
                self.api_base
            ))
            .map_err(|error| GithubError::Decode {
                context: "issue comments URL".to_owned(),
                details: error.to_string(),
            })?;
            let request = self
                .http
                .get(url)
                .query(&[("per_page", "100"), ("page", page_value.as_str())]);
            let (_, _, body) = self.send(request).await?;
            let page_items: Vec<CommentDto> = decode(&body, "issue comments")?;
            let count = page_items.len();
            comments.extend(page_items.into_iter().map(comment_from_dto));
            if count < PER_PAGE || page >= 100 {
                break;
            }
            page += 1;
        }
        Ok(comments)
    }

    pub async fn list_pulls(
        &self,
        owner: &str,
        repo: &str,
        state: &str,
    ) -> Result<Vec<PullRequest>, GithubError> {
        let mut pulls = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let url = Url::parse(&format!("{}/repos/{owner}/{repo}/pulls", self.api_base))
                .map_err(|error| GithubError::Decode {
                    context: "pull requests URL".to_owned(),
                    details: error.to_string(),
                })?;
            let request = self.http.get(url).query(&[
                ("state", state),
                ("sort", "updated"),
                ("direction", "desc"),
                ("per_page", "100"),
                ("page", page_value.as_str()),
            ]);
            let (_, _, body) = self.send(request).await?;
            let page_items: Vec<PullRequestDto> = decode(&body, "pull requests")?;
            let count = page_items.len();
            pulls.extend(
                page_items
                    .into_iter()
                    .map(|pull| pull_from_dto(pull, false)),
            );
            if count < PER_PAGE || page >= 100 {
                break;
            }
            page += 1;
        }
        Ok(pulls)
    }

    pub async fn get_pull(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<PullRequest, GithubError> {
        let url = Url::parse(&format!(
            "{}/repos/{owner}/{repo}/pulls/{number}",
            self.api_base
        ))
        .map_err(|error| GithubError::Decode {
            context: "pull request URL".to_owned(),
            details: error.to_string(),
        })?;
        let (_, _, body) = self.send(self.http.get(url)).await?;
        let dto: PullRequestDto = decode(&body, "pull request")?;
        Ok(pull_from_dto(dto, true))
    }

    pub async fn list_pull_files(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<Vec<FilePatch>, GithubError> {
        let mut files = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let url = Url::parse(&format!(
                "{}/repos/{owner}/{repo}/pulls/{number}/files",
                self.api_base
            ))
            .map_err(|error| GithubError::Decode {
                context: "pull files URL".to_owned(),
                details: error.to_string(),
            })?;
            let request = self
                .http
                .get(url)
                .query(&[("per_page", "100"), ("page", page_value.as_str())]);
            let (_, _, body) = self.send(request).await?;
            let page_items: Vec<FileDto> = decode(&body, "pull files")?;
            let count = page_items.len();
            files.extend(page_items.into_iter().map(file_from_dto));
            if count < PER_PAGE || page >= 30 {
                break;
            }
            page += 1;
        }
        Ok(files)
    }

    pub async fn list_workflow_runs(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<WorkflowRun>, GithubError> {
        let mut runs = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let url = Url::parse(&format!(
                "{}/repos/{owner}/{repo}/actions/runs",
                self.api_base
            ))
            .map_err(|error| GithubError::Decode {
                context: "workflow runs URL".to_owned(),
                details: error.to_string(),
            })?;
            let request = self
                .http
                .get(url)
                .query(&[("per_page", "100"), ("page", page_value.as_str())]);
            let (_, _, body) = self.send(request).await?;
            let page_data: WorkflowRunsDto = decode(&body, "workflow runs")?;
            let count = page_data.workflow_runs.len();
            runs.extend(page_data.workflow_runs.into_iter().map(run_from_dto));
            if count < PER_PAGE || page >= 10 {
                break;
            }
            page += 1;
        }
        Ok(runs)
    }

    pub async fn list_workflow_jobs(
        &self,
        owner: &str,
        repo: &str,
        run_id: i64,
    ) -> Result<Vec<WorkflowJob>, GithubError> {
        let mut jobs = Vec::new();
        let mut page = 1u32;
        loop {
            let page_value = page.to_string();
            let url = Url::parse(&format!(
                "{}/repos/{owner}/{repo}/actions/runs/{run_id}/jobs",
                self.api_base
            ))
            .map_err(|error| GithubError::Decode {
                context: "workflow jobs URL".to_owned(),
                details: error.to_string(),
            })?;
            let request = self.http.get(url).query(&[
                ("filter", "latest"),
                ("per_page", "100"),
                ("page", page_value.as_str()),
            ]);
            let (_, _, body) = self.send(request).await?;
            let page_data: WorkflowJobsDto = decode(&body, "workflow jobs")?;
            let count = page_data.jobs.len();
            jobs.extend(page_data.jobs.into_iter().map(job_from_dto));
            if count < PER_PAGE || page >= 100 {
                break;
            }
            page += 1;
        }
        Ok(jobs)
    }

    pub async fn get_job_logs(
        &self,
        owner: &str,
        repo: &str,
        job_id: i64,
    ) -> Result<String, GithubError> {
        let url = Url::parse(&format!(
            "{}/repos/{owner}/{repo}/actions/jobs/{job_id}/logs",
            self.api_base
        ))
        .map_err(|error| GithubError::Decode {
            context: "job logs URL".to_owned(),
            details: error.to_string(),
        })?;
        let (_, _, body) = self.send(self.http.get(url)).await?;
        let mut text = body;
        if text.len() > 2_000_000 {
            text.truncate(2_000_000);
            text.push_str("\n… log truncado …");
        }
        Ok(text)
    }

    pub async fn fetch_avatar(&self, login: &str) -> Result<Vec<u8>, GithubError> {
        let url = format!("https://github.com/{login}.png?size=128");
        let response = self
            .http
            .get(url)
            .header(ACCEPT, "image/png,image/jpeg")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(GithubError::Http {
                status: status.as_u16(),
                message: format!("could not fetch avatar for {login}"),
                request_id: None,
            });
        }
        let bytes = response.bytes().await?;
        if bytes.len() > 2_000_000 {
            return Err(GithubError::Decode {
                context: "avatar".to_owned(),
                details: "image larger than 2 MB".to_owned(),
            });
        }
        Ok(bytes.to_vec())
    }

    fn contents_url(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        reference: &str,
    ) -> Result<Url, GithubError> {
        let mut url = Url::parse(&format!("{}/repos/{owner}/{repo}/contents", self.api_base))
            .map_err(|error| GithubError::Decode {
                context: "contents URL".to_owned(),
                details: error.to_string(),
            })?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| GithubError::Decode {
                context: "contents URL".to_owned(),
                details: "URL cannot be a base URL".to_owned(),
            })?;
            for part in path.split('/').filter(|part| !part.is_empty()) {
                segments.push(part);
            }
        }
        if !reference.is_empty() {
            url.query_pairs_mut().append_pair("ref", reference);
        }
        Ok(url)
    }

    async fn send(
        &self,
        request: RequestBuilder,
    ) -> Result<(StatusCode, HeaderMap, String), GithubError> {
        let response = request
            .bearer_auth(self.token.expose())
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header(ACCEPT, "application/vnd.github+json")
            .send()
            .await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(GithubError::from_http(status, &headers, &body));
        }
        Ok((status, headers, body))
    }
}

fn repository_from_dto(dto: RepoDto) -> Repository {
    let visibility = dto
        .visibility
        .unwrap_or_else(|| (if dto.private { "private" } else { "public" }).to_owned());
    Repository {
        github_id: dto.id,
        owner: dto.owner.login,
        name: dto.name,
        full_name: dto.full_name,
        description: dto.description,
        private: dto.private,
        visibility,
        fork: dto.fork,
        archived: dto.archived,
        default_branch: dto.default_branch,
        language: dto.language,
        stars: dto.stargazers_count,
        permissions_admin: dto.permissions.map(|value| value.admin).unwrap_or(false),
        html_url: dto.html_url,
        updated_at: dto.updated_at,
        pushed_at: dto.pushed_at,
        synced_at: None,
        monitored: true,
        last_sync_error: None,
    }
}

fn file_from_dto(dto: FileDto) -> FilePatch {
    FilePatch {
        filename: dto.filename,
        previous_filename: dto.previous_filename,
        status: dto.status,
        additions: dto.additions,
        deletions: dto.deletions,
        changes: dto.changes,
        patch: dto.patch,
    }
}

fn issue_from_dto(mut dto: IssueDto, include_body: bool) -> Issue {
    if !include_body {
        dto.body = None;
    }
    Issue {
        number: dto.number,
        title: dto.title,
        body: dto.body,
        state: dto.state,
        user: dto.user.map(|user| user.login),
        labels: dto
            .labels
            .into_iter()
            .map(|label| Label {
                name: label.name,
                color: label.color,
            })
            .collect(),
        comments: dto.comments,
        created_at: dto.created_at,
        updated_at: dto.updated_at,
        closed_at: dto.closed_at,
        html_url: dto.html_url,
    }
}

fn pull_from_dto(mut dto: PullRequestDto, include_body: bool) -> PullRequest {
    if !include_body {
        dto.body = None;
    }
    PullRequest {
        number: dto.number,
        title: dto.title,
        body: dto.body,
        state: dto.state,
        merged: dto.merged,
        draft: dto.draft,
        user: dto.user.map(|user| user.login),
        head_ref: dto.head.ref_name,
        head_sha: dto.head.sha,
        base_ref: dto.base.ref_name,
        base_sha: dto.base.sha,
        comments: dto.comments,
        commits: dto.commits,
        additions: dto.additions,
        deletions: dto.deletions,
        changed_files: dto.changed_files,
        created_at: dto.created_at,
        updated_at: dto.updated_at,
        merged_at: dto.merged_at,
        html_url: dto.html_url,
    }
}

fn comment_from_dto(dto: CommentDto) -> IssueComment {
    IssueComment {
        id: dto.id,
        body: dto.body,
        user: dto.user.map(|user| user.login),
        created_at: dto.created_at,
        html_url: dto.html_url,
    }
}

fn run_from_dto(dto: WorkflowRunDto) -> WorkflowRun {
    WorkflowRun {
        id: dto.id,
        name: dto.name,
        display_title: dto.display_title,
        run_number: dto.run_number,
        event: dto.event,
        status: dto.status,
        conclusion: dto.conclusion,
        head_branch: dto.head_branch,
        head_sha: dto.head_sha,
        actor: dto.actor.map(|actor| actor.login),
        html_url: dto.html_url,
        created_at: dto.created_at,
        updated_at: dto.updated_at,
        run_attempt: dto.run_attempt,
        workflow_id: dto.workflow_id,
    }
}

fn job_from_dto(dto: WorkflowJobDto) -> WorkflowJob {
    WorkflowJob {
        id: dto.id,
        run_id: dto.run_id,
        name: dto.name,
        status: dto.status,
        conclusion: dto.conclusion,
        started_at: dto.started_at,
        completed_at: dto.completed_at,
        html_url: dto.html_url,
        runner_name: dto.runner_name,
        steps: dto
            .steps
            .into_iter()
            .map(|step| WorkflowStep {
                number: step.number,
                name: step.name,
                status: step.status,
                conclusion: step.conclusion,
            })
            .collect(),
    }
}

fn release_from_dto(dto: ReleaseDto) -> Release {
    Release {
        id: dto.id,
        tag_name: dto.tag_name,
        name: dto.name,
        body: dto.body,
        draft: dto.draft,
        prerelease: dto.prerelease,
        created_at: dto.created_at,
        published_at: dto.published_at,
        html_url: dto.html_url,
        author: dto.author.map(|author| author.login),
        assets: dto
            .assets
            .into_iter()
            .map(|asset| ReleaseAsset {
                name: asset.name,
                size: asset.size,
                download_count: asset.download_count,
                browser_download_url: asset.browser_download_url,
            })
            .collect(),
    }
}

fn content_payload_from_value(
    value: &Value,
    requested_path: &str,
) -> Result<ContentPayload, GithubError> {
    if let Some(items) = value.as_array() {
        let entries = items.iter().filter_map(content_entry_from_value).collect();
        return Ok(ContentPayload {
            path: requested_path.to_owned(),
            kind: "dir".to_owned(),
            entries,
            ..Default::default()
        });
    }

    let object = value.as_object().ok_or_else(|| GithubError::Decode {
        context: "repository contents".to_owned(),
        details: "expected an object or an array".to_owned(),
    })?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("file")
        .to_owned();
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(requested_path)
        .to_owned();
    let size = object.get("size").and_then(Value::as_u64);
    let encoded = object.get("content").and_then(Value::as_str);
    let (content, binary, truncated) = match encoded {
        Some(encoded) => {
            let compact: String = encoded.chars().filter(|ch| !ch.is_whitespace()).collect();
            match BASE64.decode(compact) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => (Some(text), false, false),
                    Err(_) => (None, true, false),
                },
                Err(_) => (None, false, true),
            }
        }
        None => (None, false, size.unwrap_or(0) > 0),
    };
    Ok(ContentPayload {
        path,
        kind,
        sha: object.get("sha").and_then(Value::as_str).map(str::to_owned),
        size,
        content,
        entries: Vec::new(),
        html_url: object
            .get("html_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
        truncated,
        binary,
    })
}

fn content_entry_from_value(value: &Value) -> Option<ContentEntry> {
    Some(ContentEntry {
        name: value.get("name")?.as_str()?.to_owned(),
        path: value.get("path")?.as_str()?.to_owned(),
        sha: value.get("sha")?.as_str()?.to_owned(),
        kind: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("file")
            .to_owned(),
        size: value.get("size").and_then(Value::as_u64),
        html_url: value
            .get("html_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
        download_url: value
            .get("download_url")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

fn parse_rate_limit(value: &Value) -> Option<RateLimit> {
    let limit = value["limit"].as_u64()? as u32;
    let remaining = value["remaining"].as_u64().unwrap_or(0) as u32;
    let reset_at = value["resetAt"]
        .as_str()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))?;
    Some(RateLimit {
        limit,
        remaining,
        reset_at,
        cost: value["cost"].as_u64().map(|value| value as u32),
    })
}

fn decode<T: serde::de::DeserializeOwned>(body: &str, context: &str) -> Result<T, GithubError> {
    serde_json::from_str(body).map_err(|error| GithubError::Decode {
        context: context.to_owned(),
        details: error.to_string(),
    })
}

const COMMIT_QUERY: &str = r#"
query($owner: String!, $name: String!, $branch: String!, $since: GitTimestamp!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    ref(qualifiedName: $branch) {
      target {
        ... on Commit {
          history(first: 100, since: $since, after: $cursor) {
            pageInfo { hasNextPage endCursor }
            nodes {
              oid
              messageHeadline
              committedDate
              additions
              deletions
              author { name email user { login } }
              committer { name email user { login } }
              parents(first: 2) { totalCount }
            }
          }
        }
      }
    }
  }
  rateLimit { limit remaining resetAt cost }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn lists_repositories() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user/repos"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                    "id": 1,
                    "name": "gitpanel",
                    "full_name": "MiguelSilvaPorto/gitpanel",
                    "owner": { "login": "MiguelSilvaPorto" },
                    "description": "Painel",
                    "private": false,
                    "visibility": "public",
                    "fork": false,
                    "archived": false,
                    "default_branch": "main",
                    "language": "Rust",
                    "stargazers_count": 4,
                    "permissions": { "admin": true },
                    "html_url": "https://github.com/MiguelSilvaPorto/gitpanel",
                    "updated_at": "2026-01-01T00:00:00Z",
                    "pushed_at": "2026-01-02T00:00:00Z"
                }])),
            )
            .mount(&server)
            .await;

        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let repos = client.list_repositories().await.expect("repos");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "MiguelSilvaPorto/gitpanel");
        assert!(repos[0].permissions_admin);
    }

    #[tokio::test]
    async fn maps_forbidden_to_rate_limit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(
                ResponseTemplate::new(403)
                    .insert_header("x-ratelimit-remaining", "0")
                    .insert_header("x-ratelimit-reset", "1770000000")
                    .set_body_string("{\"message\":\"API rate limit exceeded\"}"),
            )
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let error = client.get_account().await.expect_err("rate limited");
        assert!(error.is_rate_limited());
    }

    #[tokio::test]
    async fn creates_a_repository() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/user/repos"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "id": 42,
                "name": "new-repo",
                "full_name": "MiguelSilvaPorto/new-repo",
                "owner": { "login": "MiguelSilvaPorto" },
                "description": "created",
                "private": true,
                "visibility": "private",
                "fork": false,
                "archived": false,
                "default_branch": "main",
                "language": null,
                "stargazers_count": 0,
                "permissions": { "admin": true },
                "html_url": "https://github.com/MiguelSilvaPorto/new-repo",
                "updated_at": "2026-01-01T00:00:00Z",
                "pushed_at": null
            })))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let repository = client
            .create_repository(&CreateRepoRequest {
                name: "new-repo".to_owned(),
                description: Some("created".to_owned()),
                private: true,
                auto_init: true,
                gitignore_template: Some("Rust".to_owned()),
                license_template: Some("mit".to_owned()),
            })
            .await
            .expect("created");
        assert_eq!(repository.full_name, "MiguelSilvaPorto/new-repo");
        assert!(repository.private);
    }

    #[tokio::test]
    async fn parses_graphql_commit_pages() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "repository": {
                        "ref": {
                            "target": {
                                "history": {
                                    "pageInfo": { "hasNextPage": false, "endCursor": null },
                                    "nodes": [{
                                        "oid": "abc123",
                                        "messageHeadline": "feat: initial",
                                        "committedDate": "2026-01-02T03:04:05Z",
                                        "additions": 10,
                                        "deletions": 2,
                                        "author": { "name": "Miguel", "email": "m@example.com", "user": { "login": "miguel" } },
                                        "committer": { "name": "Miguel", "email": "m@example.com", "user": { "login": "miguel" } },
                                        "parents": { "totalCount": 1 }
                                    }]
                                }
                            }
                        }
                    },
                    "rateLimit": { "limit": 5000, "remaining": 4999, "resetAt": "2026-01-02T04:00:00Z", "cost": 1 }
                }
            })))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let page = client
            .fetch_commits_page("MiguelSilvaPorto", "gitpanel", "main", Utc::now(), None)
            .await
            .expect("page");
        assert_eq!(page.commits.len(), 1);
        assert_eq!(page.commits[0].sha, "abc123");
        assert_eq!(page.commits[0].additions, 10);
        assert_eq!(
            page.rate_limit.as_ref().map(|rate| rate.remaining),
            Some(4999)
        );
    }

    #[tokio::test]
    async fn reads_directory_contents() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/MiguelSilvaPorto/gitpanel/contents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "name": "src",
                    "path": "src",
                    "sha": "dir-sha",
                    "type": "dir",
                    "size": 0,
                    "html_url": "https://github.com/MiguelSilvaPorto/gitpanel/tree/main/src"
                },
                {
                    "name": "README.md",
                    "path": "README.md",
                    "sha": "file-sha",
                    "type": "file",
                    "size": 12,
                    "html_url": "https://github.com/MiguelSilvaPorto/gitpanel/blob/main/README.md"
                }
            ])))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let payload = client
            .get_contents("MiguelSilvaPorto", "gitpanel", "", "main")
            .await
            .expect("contents");
        assert!(payload.is_dir());
        assert_eq!(payload.entries.len(), 2);
        assert_eq!(payload.entries[0].kind, "dir");
    }

    #[tokio::test]
    async fn reads_file_contents() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/MiguelSilvaPorto/gitpanel/contents/README.md"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "README.md",
                "path": "README.md",
                "sha": "file-sha",
                "type": "file",
                "size": 5,
                "encoding": "base64",
                "content": "SGVsbG8=",
                "html_url": "https://github.com/MiguelSilvaPorto/gitpanel/blob/main/README.md"
            })))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let payload = client
            .get_contents("MiguelSilvaPorto", "gitpanel", "README.md", "main")
            .await
            .expect("file");
        assert_eq!(payload.content.as_deref(), Some("Hello"));
        assert!(!payload.binary);
    }

    #[tokio::test]
    async fn reads_a_recursive_tree() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/MiguelSilvaPorto/gitpanel/git/trees/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "tree-sha",
                "truncated": false,
                "tree": [
                    { "path": ".cargo", "mode": "040000", "type": "tree", "sha": "dir-sha" },
                    { "path": ".cargo/config.toml", "mode": "100644", "type": "blob", "sha": "file-sha", "size": 63 },
                    { "path": "vendor", "mode": "160000", "type": "commit", "sha": "submodule-sha" }
                ]
            })))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let tree = client
            .get_tree("MiguelSilvaPorto", "gitpanel", "main")
            .await
            .expect("tree");
        assert!(!tree.truncated);
        assert_eq!(tree.entries.len(), 3);
        assert_eq!(tree.entries[0].kind, "dir");
        assert_eq!(tree.entries[1].name, "config.toml");
        assert_eq!(tree.entries[1].size, Some(63));
        assert_eq!(tree.entries[2].kind, "file");
    }

    #[tokio::test]
    async fn updates_a_file_with_a_sha() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/repos/MiguelSilvaPorto/gitpanel/contents/README.md"))
            .and(body_json(serde_json::json!({
                "message": "docs: update readme",
                "content": "SGVsbG8=",
                "branch": "main",
                "sha": "file-sha"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "commit": {
                    "sha": "commit-sha",
                    "message": "docs: update readme",
                    "author": {
                        "name": "Miguel",
                        "email": "m@example.com",
                        "date": "2026-01-02T03:04:05Z"
                    }
                }
            })))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let commit = client
            .update_file(FileUpdate {
                owner: "MiguelSilvaPorto",
                repo: "gitpanel",
                path: "README.md",
                branch: "main",
                content: "Hello",
                message: "docs: update readme",
                sha: "file-sha",
            })
            .await
            .expect("updated");
        assert_eq!(commit.sha, "commit-sha");
        assert_eq!(commit.message, "docs: update readme");
        assert_eq!(commit.author_name, "Miguel");
    }

    #[tokio::test]
    async fn lists_releases() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/MiguelSilvaPorto/gitpanel/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "id": 1,
                "tag_name": "v0.1.0",
                "name": "First",
                "body": "Notes",
                "draft": false,
                "prerelease": false,
                "created_at": "2026-01-01T00:00:00Z",
                "published_at": "2026-01-02T00:00:00Z",
                "html_url": "https://github.com/MiguelSilvaPorto/gitpanel/releases/tag/v0.1.0",
                "author": { "login": "MiguelSilvaPorto" },
                "assets": [{ "name": "app", "size": 10, "download_count": 3, "browser_download_url": "https://example.com/app" }]
            }])))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let releases = client
            .list_releases("MiguelSilvaPorto", "gitpanel")
            .await
            .expect("releases");
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].tag_name, "v0.1.0");
        assert_eq!(releases[0].assets.len(), 1);
    }

    #[tokio::test]
    async fn lists_issues_without_pull_requests() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/MiguelSilvaPorto/gitpanel/issues"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 1,
                    "title": "Issue",
                    "body": "Body",
                    "state": "open",
                    "user": { "login": "alice" },
                    "labels": [{ "name": "bug", "color": "ff0000" }],
                    "comments": 2,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-02T00:00:00Z",
                    "closed_at": null,
                    "html_url": "https://github.com/MiguelSilvaPorto/gitpanel/issues/1"
                },
                {
                    "number": 2,
                    "title": "PR",
                    "body": "Body",
                    "state": "open",
                    "user": { "login": "bob" },
                    "labels": [],
                    "comments": 0,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-02T00:00:00Z",
                    "closed_at": null,
                    "html_url": "https://github.com/MiguelSilvaPorto/gitpanel/pull/2",
                    "pull_request": { "url": "https://api.github.com/pulls/2" }
                }
            ])))
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let issues = client
            .list_issues("MiguelSilvaPorto", "gitpanel", "open")
            .await
            .expect("issues");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 1);
        assert_eq!(issues[0].labels[0].name, "bug");
        assert!(issues[0].body.is_none());
    }

    #[tokio::test]
    async fn lists_pull_requests() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/MiguelSilvaPorto/gitpanel/pulls"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                    "number": 7,
                    "title": "Feature",
                    "body": "Body",
                    "state": "open",
                    "merged": false,
                    "draft": true,
                    "user": { "login": "alice" },
                    "head": { "ref": "feature", "sha": "head-sha" },
                    "base": { "ref": "main", "sha": "base-sha" },
                    "comments": 3,
                    "commits": 2,
                    "additions": 10,
                    "deletions": 2,
                    "changed_files": 1,
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-02T00:00:00Z",
                    "merged_at": null,
                    "html_url": "https://github.com/MiguelSilvaPorto/gitpanel/pull/7"
                }])),
            )
            .mount(&server)
            .await;
        let client = GithubClient::with_base(Token::new("test"), server.uri()).expect("client");
        let pulls = client
            .list_pulls("MiguelSilvaPorto", "gitpanel", "open")
            .await
            .expect("pulls");
        assert_eq!(pulls.len(), 1);
        assert_eq!(pulls[0].head_ref, "feature");
        assert!(pulls[0].draft);
        assert!(pulls[0].body.is_none());
    }
}
