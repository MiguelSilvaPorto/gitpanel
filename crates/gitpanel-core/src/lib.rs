#![forbid(unsafe_code)]

pub mod error;
pub mod models;
pub mod validation;

pub use error::ValidationError;
pub use models::{
    Account, Branch, Commit, CommitData, CommitDiffResult, CompareResult, ContentEntry,
    ContentPayload, CreateRepoRequest, FilePatch, Issue, IssueComment, Label, PullRequest,
    RateLimit, Release, ReleaseAsset, RepoTree, RepoTreeEntry, Repository, WorkflowJob,
    WorkflowRun, WorkflowStep,
};
pub use validation::{confirmation_matches, validate_description, validate_repo_name};
