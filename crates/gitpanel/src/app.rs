use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::{DateTime, TimeDelta, Utc};
use eframe::egui;
use egui::{
    Align, Align2, Color32, FontId, Frame, Id, Layout, Margin, RichText, ScrollArea, Stroke, Ui,
};
use gitpanel_config::{Config, ThemePreference};
use gitpanel_core::{
    Account, Branch, Commit, CommitData, ContentPayload, CreateRepoRequest, FilePatch, Issue,
    IssueComment, Label, PullRequest, RateLimit, Release, Repository, ValidationError, WorkflowJob,
    WorkflowRun, validate_description, validate_repo_name,
};
use gitpanel_db::Db;
use gitpanel_diff::{Diff, Highlighter};
use gitpanel_github::{GithubClient, GithubError};
use gitpanel_i18n::{Arg, LanguagePreference, Translator, keys};
use gitpanel_metrics::{Granularity, aggregate_local, summary, top_contributors};
use gitpanel_sync::{SyncEvent, SyncHandle};
use gitpanel_ui::{
    CommonMarkCache, DataSource, DiffViewState, Icon, Palette, Theme, badge, card, commit_chart,
    diff_view, empty_state, error_banner, format_bytes, format_date, icon, icon_button,
    info_banner, install_fonts, kv_row, loading, markdown, muted, outline_badge, page_header,
    panel_frame, primary_button, relative_time, repo_grid, repo_stats, repo_table, search_input,
    secondary_button, source_badge, stat_card, tabs,
};

use crate::jobs::Jobs;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Repositories,
    Repository,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepoTab {
    Overview,
    Code,
    Commits,
    Branches,
    Releases,
    Issues,
    Pulls,
    Actions,
    Charts,
    Diff,
}

impl RepoTab {
    fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Code => 1,
            Self::Commits => 2,
            Self::Branches => 3,
            Self::Releases => 4,
            Self::Issues => 5,
            Self::Pulls => 6,
            Self::Actions => 7,
            Self::Charts => 8,
            Self::Diff => 9,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Code,
            2 => Self::Commits,
            3 => Self::Branches,
            4 => Self::Releases,
            5 => Self::Issues,
            6 => Self::Pulls,
            7 => Self::Actions,
            8 => Self::Charts,
            9 => Self::Diff,
            _ => Self::Overview,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepoFilter {
    All,
    Own,
    Others,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RepoView {
    Grid,
    Table,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffSource {
    Commit,
    Compare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToastKind {
    Success,
    Error,
    Info,
}

struct Toast {
    text: String,
    kind: ToastKind,
    created: Instant,
}

#[derive(Clone)]
struct CreateForm {
    name: String,
    description: String,
    private: bool,
    auto_init: bool,
    gitignore: String,
    license: String,
    error: Option<String>,
}

impl Default for CreateForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            private: false,
            auto_init: true,
            gitignore: String::new(),
            license: String::new(),
            error: None,
        }
    }
}

#[derive(Clone)]
struct SettingsForm {
    theme: ThemePreference,
    language: LanguagePreference,
    interval_minutes: u64,
    backfill_days: u32,
    sync_all: bool,
    cache_enabled: bool,
    cache_keep_days: u32,
    cache_max_file_kb: u64,
}

impl SettingsForm {
    fn from_config(config: &Config) -> Self {
        Self {
            theme: config.settings.ui.theme,
            language: config.settings.ui.language.clone(),
            interval_minutes: config.settings.sync.interval_minutes,
            backfill_days: config.settings.sync.backfill_days,
            sync_all: config.settings.sync.sync_all_accessible,
            cache_enabled: config.settings.cache.enabled,
            cache_keep_days: config.settings.cache.keep_days,
            cache_max_file_kb: config.settings.cache.max_file_kb,
        }
    }
}

#[derive(Default)]
struct SyncUiState {
    running: bool,
    paused: bool,
    total: usize,
    current: usize,
    current_repo_id: Option<i64>,
    current_repo_name: Option<String>,
    added: u32,
    errors: usize,
    last_sync: Option<DateTime<Utc>>,
    rate_limit: Option<RateLimit>,
    paused_until: Option<DateTime<Utc>>,
    error: Option<String>,
}

struct ChartsState {
    commits: Vec<Commit>,
    repo_id: Option<i64>,
    period_days: Option<u32>,
    granularity: Granularity,
    include_merges: bool,
}

impl Default for ChartsState {
    fn default() -> Self {
        Self {
            commits: Vec::new(),
            repo_id: None,
            period_days: Some(30),
            granularity: Granularity::Day,
            include_merges: true,
        }
    }
}

struct DiffLoad {
    files: Vec<FilePatch>,
    label: String,
}

struct BranchCommitsJob {
    repo_id: i64,
    branch: String,
    commits: Vec<CommitData>,
}

pub struct GitPanelApp {
    config: Config,
    translator: Translator,
    theme: Theme,
    db: Db,
    jobs: Jobs,
    client: Option<GithubClient>,
    account: Option<Account>,
    account_pending: Option<Receiver<Result<Account, GithubError>>>,
    auth_error: Option<String>,
    repos: Vec<Repository>,
    sync: Option<SyncHandle>,
    sync_state: SyncUiState,
    screen: Screen,
    scroll_generation: u64,
    selected_repo_id: Option<i64>,
    repo_tab: RepoTab,
    repo_search: String,
    repo_filter: RepoFilter,
    repo_view: RepoView,
    commits: Vec<Commit>,
    commits_total: u32,
    commits_repo: Option<i64>,
    commits_branch: String,
    commits_source: DataSource,
    commits_pending: Option<Receiver<Result<BranchCommitsJob, GithubError>>>,
    commits_pending_repo: Option<i64>,
    commits_pending_branch: Option<String>,
    commits_error: Option<String>,
    selected_branch: String,
    charts: ChartsState,
    branches: Vec<Branch>,
    branches_repo: Option<i64>,
    branches_pending: Option<Receiver<Result<Vec<Branch>, GithubError>>>,
    branches_source: DataSource,
    branches_error: Option<String>,
    code_path: String,
    contents: Option<ContentPayload>,
    contents_repo: Option<i64>,
    contents_branch: String,
    contents_source: DataSource,
    contents_pending: Option<Receiver<Result<ContentPayload, GithubError>>>,
    contents_error: Option<String>,
    readme: Option<ContentPayload>,
    readme_repo: Option<i64>,
    readme_branch: String,
    readme_source: DataSource,
    readme_pending: Option<Receiver<Result<Option<ContentPayload>, GithubError>>>,
    readme_error: Option<String>,
    code_file: Option<ContentPayload>,
    code_file_path: Option<String>,
    file_source: DataSource,
    file_pending: Option<Receiver<Result<ContentPayload, GithubError>>>,
    file_error: Option<String>,
    releases: Vec<Release>,
    releases_repo: Option<i64>,
    releases_source: DataSource,
    releases_pending: Option<Receiver<Result<Vec<Release>, GithubError>>>,
    releases_error: Option<String>,
    issues_state: String,
    issues: Vec<Issue>,
    issues_repo: Option<i64>,
    issues_source: DataSource,
    issues_pending: Option<Receiver<Result<Vec<Issue>, GithubError>>>,
    issues_error: Option<String>,
    selected_issue: Option<Issue>,
    issue_detail_source: DataSource,
    issue_detail_pending: Option<Receiver<Result<Issue, GithubError>>>,
    issue_detail_error: Option<String>,
    issue_comments: Vec<IssueComment>,
    issue_comments_source: DataSource,
    issue_comments_pending: Option<Receiver<Result<Vec<IssueComment>, GithubError>>>,
    issue_comments_error: Option<String>,
    pulls_state: String,
    pulls: Vec<PullRequest>,
    pulls_repo: Option<i64>,
    pulls_source: DataSource,
    pulls_pending: Option<Receiver<Result<Vec<PullRequest>, GithubError>>>,
    pulls_error: Option<String>,
    selected_pull: Option<PullRequest>,
    pull_detail_source: DataSource,
    pull_detail_pending: Option<Receiver<Result<PullRequest, GithubError>>>,
    pull_detail_error: Option<String>,
    pull_comments: Vec<IssueComment>,
    pull_comments_source: DataSource,
    pull_comments_pending: Option<Receiver<Result<Vec<IssueComment>, GithubError>>>,
    pull_comments_error: Option<String>,
    pull_files: Vec<FilePatch>,
    pull_files_source: DataSource,
    pull_files_pending: Option<Receiver<Result<Vec<FilePatch>, GithubError>>>,
    pull_files_error: Option<String>,
    pull_diff: Option<Diff>,
    pull_diff_state: DiffViewState,
    actions_filter: String,
    actions_runs: Vec<WorkflowRun>,
    actions_repo: Option<i64>,
    actions_source: DataSource,
    actions_pending: Option<Receiver<Result<Vec<WorkflowRun>, GithubError>>>,
    actions_error: Option<String>,
    selected_run: Option<WorkflowRun>,
    action_jobs: Vec<WorkflowJob>,
    action_jobs_source: DataSource,
    action_jobs_pending: Option<Receiver<Result<Vec<WorkflowJob>, GithubError>>>,
    action_jobs_error: Option<String>,
    action_logs: HashMap<i64, String>,
    action_log_job: Option<i64>,
    action_log_source: DataSource,
    action_log_pending: Option<Receiver<Result<(i64, String), GithubError>>>,
    action_log_error: Option<String>,
    markdown_cache: CommonMarkCache,
    diff_source: DiffSource,
    diff_state: DiffViewState,
    diff: Option<Diff>,
    diff_label: String,
    diff_loading: bool,
    diff_error: Option<String>,
    diff_pending: Option<Receiver<Result<DiffLoad, GithubError>>>,
    highlighter: Option<Highlighter>,
    selected_commit: Option<String>,
    base_branch: String,
    head_branch: String,
    create_open: bool,
    create_form: CreateForm,
    create_pending: Option<Receiver<Result<Repository, GithubError>>>,
    settings: SettingsForm,
    toasts: Vec<Toast>,
}

impl GitPanelApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        install_fonts(&cc.egui_ctx);
        let config = Config::load().context("could not load configuration")?;
        let translator =
            Translator::new(&config.settings.ui.language).context("could not load translations")?;
        let db = Db::open(config.paths.database_file.clone())
            .context("could not open the local database")?;
        if let Err(error) = db.prune_cache(config.settings.cache.keep_days) {
            tracing::warn!(%error, "could not prune old cache entries");
        }
        let jobs = Jobs::new().context("could not start the background runtime")?;
        let system_dark = cc
            .egui_ctx
            .system_theme()
            .map(|theme| theme == egui::Theme::Dark);
        let theme = Theme::resolve(config.settings.ui.theme, system_dark);
        theme.apply(&cc.egui_ctx);

        let repositories = db.list_repositories().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not load cached repositories");
            Vec::new()
        });
        let settings = SettingsForm::from_config(&config);
        let mut app = Self {
            config,
            translator,
            theme,
            db,
            jobs,
            client: None,
            account: None,
            account_pending: None,
            auth_error: None,
            repos: repositories,
            sync: None,
            sync_state: SyncUiState::default(),
            screen: Screen::Repositories,
            scroll_generation: 0,
            selected_repo_id: None,
            repo_tab: RepoTab::Overview,
            repo_search: String::new(),
            repo_filter: RepoFilter::All,
            repo_view: RepoView::Grid,
            commits: Vec::new(),
            commits_total: 0,
            commits_repo: None,
            commits_branch: String::new(),
            commits_source: DataSource::Remote,
            commits_pending: None,
            commits_pending_repo: None,
            commits_pending_branch: None,
            commits_error: None,
            selected_branch: String::new(),
            charts: ChartsState::default(),
            branches: Vec::new(),
            branches_repo: None,
            branches_pending: None,
            branches_source: DataSource::Remote,
            branches_error: None,
            code_path: String::new(),
            contents: None,
            contents_repo: None,
            contents_branch: String::new(),
            contents_source: DataSource::Remote,
            contents_pending: None,
            contents_error: None,
            readme: None,
            readme_repo: None,
            readme_branch: String::new(),
            readme_source: DataSource::Remote,
            readme_pending: None,
            readme_error: None,
            code_file: None,
            code_file_path: None,
            file_source: DataSource::Remote,
            file_pending: None,
            file_error: None,
            releases: Vec::new(),
            releases_repo: None,
            releases_source: DataSource::Remote,
            releases_pending: None,
            releases_error: None,
            issues_state: "open".to_owned(),
            issues: Vec::new(),
            issues_repo: None,
            issues_source: DataSource::Remote,
            issues_pending: None,
            issues_error: None,
            selected_issue: None,
            issue_detail_source: DataSource::Remote,
            issue_detail_pending: None,
            issue_detail_error: None,
            issue_comments: Vec::new(),
            issue_comments_source: DataSource::Remote,
            issue_comments_pending: None,
            issue_comments_error: None,
            pulls_state: "open".to_owned(),
            pulls: Vec::new(),
            pulls_repo: None,
            pulls_source: DataSource::Remote,
            pulls_pending: None,
            pulls_error: None,
            selected_pull: None,
            pull_detail_source: DataSource::Remote,
            pull_detail_pending: None,
            pull_detail_error: None,
            pull_comments: Vec::new(),
            pull_comments_source: DataSource::Remote,
            pull_comments_pending: None,
            pull_comments_error: None,
            pull_files: Vec::new(),
            pull_files_source: DataSource::Remote,
            pull_files_pending: None,
            pull_files_error: None,
            pull_diff: None,
            pull_diff_state: DiffViewState::default(),
            actions_filter: "all".to_owned(),
            actions_runs: Vec::new(),
            actions_repo: None,
            actions_source: DataSource::Remote,
            actions_pending: None,
            actions_error: None,
            selected_run: None,
            action_jobs: Vec::new(),
            action_jobs_source: DataSource::Remote,
            action_jobs_pending: None,
            action_jobs_error: None,
            action_logs: HashMap::new(),
            action_log_job: None,
            action_log_source: DataSource::Remote,
            action_log_pending: None,
            action_log_error: None,
            markdown_cache: CommonMarkCache::default(),
            diff_source: DiffSource::Commit,
            diff_state: DiffViewState::default(),
            diff: None,
            diff_label: String::new(),
            diff_loading: false,
            diff_error: None,
            diff_pending: None,
            highlighter: None,
            selected_commit: None,
            base_branch: String::new(),
            head_branch: String::new(),
            create_open: false,
            create_form: CreateForm::default(),
            create_pending: None,
            settings,
            toasts: Vec::new(),
        };
        app.initialize_auth();
        #[cfg(debug_assertions)]
        if std::env::var("GITPANEL_OPEN_SETTINGS").is_ok() {
            app.screen = Screen::Settings;
            app.scroll_generation = app.scroll_generation.wrapping_add(1);
        }
        #[cfg(debug_assertions)]
        if std::env::var("GITPANEL_OPEN_CREATE").is_ok() {
            app.open_create();
        }
        #[cfg(debug_assertions)]
        if let Ok(full_name) = std::env::var("GITPANEL_OPEN_REPO")
            && let Some(repository) = app
                .repos
                .iter()
                .find(|repository| repository.full_name.eq_ignore_ascii_case(&full_name))
                .cloned()
        {
            app.open_repository(repository.github_id);
            match std::env::var("GITPANEL_OPEN_TAB").as_deref() {
                Ok("code") => {
                    app.repo_tab = RepoTab::Code;
                    app.ensure_code(&repository);
                }
                Ok("commits") => {
                    app.repo_tab = RepoTab::Commits;
                    app.ensure_commits(&repository);
                }
                Ok("branches") => {
                    app.repo_tab = RepoTab::Branches;
                    app.ensure_branches(&repository);
                }
                Ok("releases") => {
                    app.repo_tab = RepoTab::Releases;
                    app.load_releases(&repository, false);
                }
                Ok("issues") => {
                    app.repo_tab = RepoTab::Issues;
                    app.load_issues(&repository, &app.issues_state.clone(), false);
                }
                Ok("pulls") => {
                    app.repo_tab = RepoTab::Pulls;
                    app.load_pulls(&repository, &app.pulls_state.clone(), false);
                }
                Ok("actions") => {
                    app.repo_tab = RepoTab::Actions;
                    app.load_actions(&repository, false);
                }
                Ok("charts") => {
                    app.repo_tab = RepoTab::Charts;
                    app.load_charts(repository.github_id);
                }
                Ok("diff") => {
                    app.repo_tab = RepoTab::Diff;
                    app.ensure_branches(&repository);
                    app.ensure_commits(&repository);
                    if std::env::var("GITPANEL_AUTO_DIFF").is_ok() {
                        app.start_diff();
                    }
                }
                _ => {}
            }
            #[cfg(debug_assertions)]
            if let Ok(path) = std::env::var("GITPANEL_OPEN_PATH") {
                let branch = app.selected_branch.clone();
                app.load_file(&repository, &branch, &path, false);
            }
        }
        Ok(app)
    }

    fn initialize_auth(&mut self) {
        match GithubClient::from_gh() {
            Ok(client) => {
                self.client = Some(client.clone());
                self.auth_error = None;
                self.start_account_fetch(client.clone());
                self.start_sync();
            }
            Err(error) => {
                self.auth_error = Some(error.to_string());
                let message = self.github_error_title(&error);
                self.toast(message, ToastKind::Error);
            }
        }
    }

    fn start_account_fetch(&mut self, client: GithubClient) {
        self.account_pending = Some(self.jobs.spawn(async move { client.get_account().await }));
    }

    fn reload_auth(&mut self) {
        match GithubClient::from_gh() {
            Ok(client) => {
                self.client = Some(client.clone());
                self.auth_error = None;
                self.account = None;
                self.start_account_fetch(client);
                self.start_sync();
                self.toast(
                    self.translator.text(keys::COMMON_ACCOUNT),
                    ToastKind::Success,
                );
            }
            Err(error) => {
                self.auth_error = Some(error.to_string());
                let message = self.github_error_title(&error);
                self.toast(message, ToastKind::Error);
            }
        }
    }

    fn start_sync(&mut self) {
        if let Some(old) = self.sync.take() {
            drop(old);
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        match SyncHandle::spawn(self.db.clone(), client, self.config.settings.sync.clone()) {
            Ok(handle) => self.sync = Some(handle),
            Err(error) => {
                self.sync_state.error = Some(error.to_string());
                let message = self.translator.text(keys::ERROR_SYNC);
                self.toast(message, ToastKind::Error);
            }
        }
    }

    fn poll_background(&mut self) {
        if let Some(receiver) = self.account_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(account)) => {
                    self.account = Some(account);
                    self.account_pending = None;
                }
                Ok(Err(error)) => {
                    self.auth_error = Some(error.to_string());
                    let message = self.github_error_title(&error);
                    self.toast(message, ToastKind::Error);
                    self.account_pending = None;
                }
                Err(TryRecvError::Empty) => self.account_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => self.account_pending = None,
            }
        }

        if let Some(receiver) = self.create_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(repository)) => {
                    self.reload_repositories();
                    self.open_repository(repository.github_id);
                    self.sync_repository(repository.github_id);
                    self.create_open = false;
                    self.create_form = CreateForm::default();
                    self.create_pending = None;
                    self.toast(
                        self.translator.text(keys::REPOS_CREATE_SUCCESS),
                        ToastKind::Success,
                    );
                }
                Ok(Err(error)) => {
                    self.create_form.error = Some(self.github_error_title(&error));
                    self.create_pending = None;
                }
                Err(TryRecvError::Empty) => self.create_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.create_form.error = Some(self.translator.text(keys::ERROR_CREATE));
                    self.create_pending = None;
                }
            }
        }

        if let Some(receiver) = self.branches_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(branches)) => {
                    self.branches = branches;
                    self.branches_source = DataSource::Local;
                    self.branches_error = None;
                    self.branches_pending = None;
                    self.select_default_branches();
                }
                Ok(Err(error)) => {
                    self.branches_source = DataSource::Remote;
                    self.branches_error = Some(self.github_error_title(&error));
                    self.branches_pending = None;
                }
                Err(TryRecvError::Empty) => self.branches_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.branches_source = DataSource::Remote;
                    self.branches_pending = None;
                }
            }
        }

        if let Some(receiver) = self.releases_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(releases)) => {
                    if let Some(repo_id) = self.releases_repo
                        && let Ok(payload) = serde_json::to_string(&releases)
                    {
                        self.cache_release(repo_id, &payload);
                    }
                    self.releases = releases;
                    self.releases_source = DataSource::Local;
                    self.releases_error = None;
                    self.releases_pending = None;
                }
                Ok(Err(error)) => {
                    self.releases_source = DataSource::Remote;
                    self.releases_error = Some(self.github_error_title(&error));
                    self.releases_pending = None;
                }
                Err(TryRecvError::Empty) => self.releases_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.releases_source = DataSource::Remote;
                    self.releases_pending = None;
                }
            }
        }

        if let Some(receiver) = self.issues_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(issues)) => {
                    if let Some(repo_id) = self.issues_repo
                        && let Ok(payload) = serde_json::to_string(&issues)
                    {
                        self.cache_api(repo_id, "issues", &self.issues_state.clone(), &payload);
                    }
                    self.issues = issues;
                    self.issues_source = DataSource::Local;
                    self.issues_error = None;
                    self.issues_pending = None;
                }
                Ok(Err(error)) => {
                    self.issues_source = DataSource::Remote;
                    self.issues_error = Some(self.github_error_title(&error));
                    self.issues_pending = None;
                }
                Err(TryRecvError::Empty) => self.issues_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.issues_source = DataSource::Remote;
                    self.issues_pending = None;
                }
            }
        }

        if let Some(receiver) = self.pulls_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(pulls)) => {
                    if let Some(repo_id) = self.pulls_repo
                        && let Ok(payload) = serde_json::to_string(&pulls)
                    {
                        self.cache_api(repo_id, "pulls", &self.pulls_state.clone(), &payload);
                    }
                    self.pulls = pulls;
                    self.pulls_source = DataSource::Local;
                    self.pulls_error = None;
                    self.pulls_pending = None;
                }
                Ok(Err(error)) => {
                    self.pulls_source = DataSource::Remote;
                    self.pulls_error = Some(self.github_error_title(&error));
                    self.pulls_pending = None;
                }
                Err(TryRecvError::Empty) => self.pulls_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.pulls_source = DataSource::Remote;
                    self.pulls_pending = None;
                }
            }
        }

        if let Some(receiver) = self.issue_detail_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(detail)) => {
                    if let (Some(repo_id), Some(selected)) =
                        (self.selected_repo_id, self.selected_issue.as_ref())
                        && selected.number == detail.number
                    {
                        if let Ok(payload) = serde_json::to_string(&detail) {
                            self.cache_api(repo_id, "issue", &detail.number.to_string(), &payload);
                        }
                        self.selected_issue = Some(detail);
                        self.issue_detail_source = DataSource::Local;
                        self.issue_detail_error = None;
                    }
                    self.issue_detail_pending = None;
                }
                Ok(Err(error)) => {
                    self.issue_detail_source = DataSource::Remote;
                    self.issue_detail_error = Some(self.github_error_title(&error));
                    self.issue_detail_pending = None;
                }
                Err(TryRecvError::Empty) => self.issue_detail_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.issue_detail_source = DataSource::Remote;
                    self.issue_detail_pending = None;
                }
            }
        }

        if let Some(receiver) = self.issue_comments_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(comments)) => {
                    if let (Some(repo_id), Some(selected)) =
                        (self.selected_repo_id, self.selected_issue.as_ref())
                        && let Ok(payload) = serde_json::to_string(&comments)
                    {
                        self.cache_api(
                            repo_id,
                            "issue-comments",
                            &selected.number.to_string(),
                            &payload,
                        );
                    }
                    self.issue_comments = comments;
                    self.issue_comments_source = DataSource::Local;
                    self.issue_comments_error = None;
                    self.issue_comments_pending = None;
                }
                Ok(Err(error)) => {
                    self.issue_comments_source = DataSource::Remote;
                    self.issue_comments_error = Some(self.github_error_title(&error));
                    self.issue_comments_pending = None;
                }
                Err(TryRecvError::Empty) => self.issue_comments_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.issue_comments_source = DataSource::Remote;
                    self.issue_comments_pending = None;
                }
            }
        }

        if let Some(receiver) = self.pull_detail_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(detail)) => {
                    if let (Some(repo_id), Some(selected)) =
                        (self.selected_repo_id, self.selected_pull.as_ref())
                        && selected.number == detail.number
                    {
                        if let Ok(payload) = serde_json::to_string(&detail) {
                            self.cache_api(repo_id, "pull", &detail.number.to_string(), &payload);
                        }
                        self.selected_pull = Some(detail);
                        self.pull_detail_source = DataSource::Local;
                        self.pull_detail_error = None;
                    }
                    self.pull_detail_pending = None;
                }
                Ok(Err(error)) => {
                    self.pull_detail_source = DataSource::Remote;
                    self.pull_detail_error = Some(self.github_error_title(&error));
                    self.pull_detail_pending = None;
                }
                Err(TryRecvError::Empty) => self.pull_detail_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.pull_detail_source = DataSource::Remote;
                    self.pull_detail_pending = None;
                }
            }
        }

        if let Some(receiver) = self.pull_comments_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(comments)) => {
                    if let (Some(repo_id), Some(selected)) =
                        (self.selected_repo_id, self.selected_pull.as_ref())
                        && let Ok(payload) = serde_json::to_string(&comments)
                    {
                        self.cache_api(
                            repo_id,
                            "pull-comments",
                            &selected.number.to_string(),
                            &payload,
                        );
                    }
                    self.pull_comments = comments;
                    self.pull_comments_source = DataSource::Local;
                    self.pull_comments_error = None;
                    self.pull_comments_pending = None;
                }
                Ok(Err(error)) => {
                    self.pull_comments_source = DataSource::Remote;
                    self.pull_comments_error = Some(self.github_error_title(&error));
                    self.pull_comments_pending = None;
                }
                Err(TryRecvError::Empty) => self.pull_comments_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.pull_comments_source = DataSource::Remote;
                    self.pull_comments_pending = None;
                }
            }
        }

        if let Some(receiver) = self.pull_files_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(files)) => {
                    if let (Some(repo_id), Some(selected)) =
                        (self.selected_repo_id, self.selected_pull.as_ref())
                        && let Ok(payload) = serde_json::to_string(&files)
                    {
                        self.cache_api(
                            repo_id,
                            "pull-files",
                            &selected.number.to_string(),
                            &payload,
                        );
                    }
                    self.pull_files = files;
                    self.pull_diff = Some(Diff::from_patches(&self.pull_files));
                    self.pull_files_source = DataSource::Local;
                    self.pull_files_error = None;
                    self.pull_diff_state = DiffViewState::default();
                    self.pull_files_pending = None;
                }
                Ok(Err(error)) => {
                    self.pull_files_source = DataSource::Remote;
                    self.pull_files_error = Some(self.github_error_title(&error));
                    self.pull_files_pending = None;
                }
                Err(TryRecvError::Empty) => self.pull_files_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.pull_files_source = DataSource::Remote;
                    self.pull_files_pending = None;
                }
            }
        }

        if let Some(receiver) = self.actions_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(runs)) => {
                    if let Some(repo_id) = self.actions_repo
                        && let Ok(payload) = serde_json::to_string(&runs)
                    {
                        self.cache_api(repo_id, "actions-runs", "all", &payload);
                    }
                    self.actions_runs = runs;
                    self.actions_source = DataSource::Local;
                    self.actions_error = None;
                    self.actions_pending = None;
                }
                Ok(Err(error)) => {
                    self.actions_source = DataSource::Remote;
                    self.actions_error = Some(self.github_error_title(&error));
                    self.actions_pending = None;
                }
                Err(TryRecvError::Empty) => self.actions_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.actions_source = DataSource::Remote;
                    self.actions_pending = None;
                }
            }
        }

        if let Some(receiver) = self.action_jobs_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(jobs)) => {
                    if let (Some(repo_id), Some(run)) =
                        (self.selected_repo_id, self.selected_run.as_ref())
                        && let Ok(payload) = serde_json::to_string(&jobs)
                    {
                        self.cache_api(repo_id, "actions-jobs", &run.id.to_string(), &payload);
                    }
                    self.action_jobs = jobs;
                    self.action_jobs_source = DataSource::Local;
                    self.action_jobs_error = None;
                    self.action_jobs_pending = None;
                }
                Ok(Err(error)) => {
                    self.action_jobs_source = DataSource::Remote;
                    self.action_jobs_error = Some(self.github_error_title(&error));
                    self.action_jobs_pending = None;
                }
                Err(TryRecvError::Empty) => self.action_jobs_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.action_jobs_source = DataSource::Remote;
                    self.action_jobs_pending = None;
                }
            }
        }

        if let Some(receiver) = self.action_log_pending.take() {
            match receiver.try_recv() {
                Ok(Ok((job_id, log))) => {
                    self.action_logs.insert(job_id, log);
                    self.action_log_source = DataSource::Local;
                    self.action_log_error = None;
                    self.action_log_pending = None;
                }
                Ok(Err(error)) => {
                    self.action_log_source = DataSource::Remote;
                    self.action_log_error = Some(self.github_error_title(&error));
                    self.action_log_pending = None;
                }
                Err(TryRecvError::Empty) => self.action_log_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.action_log_source = DataSource::Remote;
                    self.action_log_pending = None;
                }
            }
        }

        if let Some(receiver) = self.contents_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(payload)) => {
                    if let (Some(repo_id), branch, path) = (
                        self.contents_repo,
                        self.contents_branch.clone(),
                        payload.path.clone(),
                    ) && let Ok(serialized) = serde_json::to_string(&payload)
                    {
                        self.cache_content(repo_id, &branch, &path, &serialized);
                    }
                    self.contents = Some(payload);
                    self.contents_source = DataSource::Local;
                    self.contents_error = None;
                    self.contents_pending = None;
                }
                Ok(Err(error)) => {
                    self.contents_source = DataSource::Remote;
                    self.contents_error = Some(self.github_error_title(&error));
                    self.contents_pending = None;
                }
                Err(TryRecvError::Empty) => self.contents_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.contents_source = DataSource::Remote;
                    self.contents_pending = None;
                }
            }
        }

        if let Some(receiver) = self.readme_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(readme)) => {
                    if let (Some(repo_id), Some(payload)) = (self.readme_repo, readme.as_ref())
                        && let Ok(serialized) = serde_json::to_string(payload)
                    {
                        self.cache_content(repo_id, &self.readme_branch, "__readme__", &serialized);
                    }
                    self.readme = readme;
                    self.readme_source = DataSource::Local;
                    self.readme_error = None;
                    self.readme_pending = None;
                }
                Ok(Err(error)) => {
                    self.readme_source = DataSource::Remote;
                    self.readme_error = Some(self.github_error_title(&error));
                    self.readme_pending = None;
                }
                Err(TryRecvError::Empty) => self.readme_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.readme_source = DataSource::Remote;
                    self.readme_pending = None;
                }
            }
        }

        if let Some(receiver) = self.file_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(payload)) => {
                    if let (Some(repo_id), Some(path)) =
                        (self.selected_repo_id, self.code_file_path.clone())
                        && let Ok(serialized) = serde_json::to_string(&payload)
                    {
                        self.cache_content(repo_id, &self.selected_branch, &path, &serialized);
                    }
                    self.code_file = Some(payload);
                    self.file_source = DataSource::Local;
                    self.file_error = None;
                    self.file_pending = None;
                }
                Ok(Err(error)) => {
                    self.file_source = DataSource::Remote;
                    self.file_error = Some(self.github_error_title(&error));
                    self.file_pending = None;
                }
                Err(TryRecvError::Empty) => self.file_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.file_source = DataSource::Remote;
                    self.file_pending = None;
                }
            }
        }

        if let Some(receiver) = self.commits_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(job)) => {
                    if let Err(error) =
                        self.db
                            .upsert_commits(job.repo_id, &job.branch, &job.commits)
                    {
                        self.commits_error =
                            Some(format!("{}: {error}", self.translator.text(keys::ERROR_DB)));
                    }
                    if self.selected_repo_id == Some(job.repo_id)
                        && self.selected_branch == job.branch
                    {
                        self.load_commits(job.repo_id, &job.branch);
                        self.commits_source = DataSource::Local;
                    }
                    self.commits_pending = None;
                    self.commits_pending_repo = None;
                    self.commits_pending_branch = None;
                }
                Ok(Err(error)) => {
                    self.commits_source = DataSource::Remote;
                    self.commits_error = Some(self.github_error_title(&error));
                    self.commits_pending = None;
                    self.commits_pending_repo = None;
                    self.commits_pending_branch = None;
                }
                Err(TryRecvError::Empty) => self.commits_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.commits_source = DataSource::Remote;
                    self.commits_pending = None;
                    self.commits_pending_repo = None;
                    self.commits_pending_branch = None;
                }
            }
        }

        if let Some(receiver) = self.diff_pending.take() {
            match receiver.try_recv() {
                Ok(Ok(load)) => {
                    self.diff = Some(Diff::from_patches(&load.files));
                    self.diff_label = load.label;
                    self.diff_error = None;
                    self.diff_loading = false;
                    self.diff_state.selected_file = 0;
                    self.highlighter = Some(Highlighter::new(self.theme.dark));
                    self.diff_pending = None;
                }
                Ok(Err(error)) => {
                    self.diff_error = Some(self.github_error_title(&error));
                    self.diff_loading = false;
                    self.diff_pending = None;
                }
                Err(TryRecvError::Empty) => self.diff_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.diff_error = Some(self.translator.text(keys::ERROR_DIFF));
                    self.diff_loading = false;
                    self.diff_pending = None;
                }
            }
        }

        if let Some(sync) = &self.sync {
            let events: Vec<_> = sync.try_recv_events();
            for event in events {
                self.handle_sync_event(event);
            }
        }

        if let Some(until) = self.sync_state.paused_until
            && Utc::now() >= until
        {
            self.sync_state.paused_until = None;
        }

        self.toasts
            .retain(|toast| toast.created.elapsed() < Duration::from_secs(6));
    }

    fn handle_sync_event(&mut self, event: SyncEvent) {
        match event {
            SyncEvent::LoadingRepositories => {
                self.sync_state.running = true;
                self.sync_state.current_repo_name =
                    Some(self.translator.text(keys::SYNC_LOADING_REPOS));
            }
            SyncEvent::RepositoriesUpdated => self.reload_repositories(),
            SyncEvent::Started { total } => {
                self.sync_state.running = true;
                self.sync_state.total = total;
                self.sync_state.current = 0;
                self.sync_state.added = 0;
                self.sync_state.errors = 0;
                self.sync_state.error = None;
            }
            SyncEvent::RepoStarted { repo_id, full_name } => {
                self.sync_state.current_repo_id = Some(repo_id);
                self.sync_state.current_repo_name = Some(full_name);
            }
            SyncEvent::RepoFinished {
                repo_id,
                added,
                error,
            } => {
                if self.sync_state.current_repo_id == Some(repo_id) {
                    self.sync_state.current_repo_id = None;
                }
                self.sync_state.added = self.sync_state.added.saturating_add(added);
                if error.is_some() {
                    self.sync_state.errors = self.sync_state.errors.saturating_add(1);
                }
            }
            SyncEvent::Progress { current, total } => {
                self.sync_state.current = current;
                self.sync_state.total = total;
            }
            SyncEvent::RateLimit {
                limit,
                remaining,
                reset_at,
            } => {
                self.sync_state.rate_limit = Some(RateLimit {
                    limit,
                    remaining,
                    reset_at,
                    cost: None,
                });
            }
            SyncEvent::RateLimited { reset_at } => {
                self.sync_state.running = false;
                self.sync_state.paused_until = Some(reset_at);
                let time = relative_time(&self.translator, reset_at, Utc::now());
                let text = self
                    .translator
                    .text_args(keys::SYNC_RATE_LIMITED_UNTIL, &[Arg::str("time", &time)]);
                self.toast(text, ToastKind::Info);
            }
            SyncEvent::Finished {
                total,
                added,
                errors,
            } => {
                self.sync_state.running = false;
                self.sync_state.current_repo_id = None;
                self.sync_state.current_repo_name = None;
                self.sync_state.total = total;
                self.sync_state.added = added;
                self.sync_state.errors = errors;
                self.sync_state.last_sync = Some(Utc::now());
                self.reload_repositories();
                self.reload_detail_data();
                if errors == 0 {
                    self.toast(self.translator.text(keys::SYNC_DONE), ToastKind::Success);
                } else {
                    let text = self.translator.text_args(
                        keys::SYNC_FINISHED_ERRORS,
                        &[Arg::int("count", errors as i64)],
                    );
                    self.toast(text, ToastKind::Error);
                }
            }
            SyncEvent::Paused(paused) => {
                self.sync_state.paused = paused;
            }
            SyncEvent::Error { code, message } => {
                self.sync_state.running = false;
                self.sync_state.error = Some(message);
                self.toast(
                    self.translator.text(translation_key_for_code(&code)),
                    ToastKind::Error,
                );
            }
        }
    }

    fn reload_repositories(&mut self) {
        match self.db.list_repositories() {
            Ok(repos) => self.repos = repos,
            Err(error) => tracing::warn!(%error, "could not reload repositories"),
        }
    }

    fn reload_detail_data(&mut self) {
        if let Some(repo_id) = self.selected_repo_id {
            if self.commits_repo == Some(repo_id) {
                self.load_commits(repo_id, &self.commits_branch.clone());
            }
            if self.charts.repo_id == Some(repo_id) {
                self.load_charts(repo_id);
            }
        }
    }

    fn load_commits(&mut self, repo_id: i64, branch: &str) {
        match self.db.list_commits_for_branch(repo_id, branch, 100, 0) {
            Ok(commits) => {
                self.commits = commits;
                self.commits_total = self
                    .db
                    .count_commits_for_branch(repo_id, branch)
                    .unwrap_or(0);
                self.commits_repo = Some(repo_id);
                self.commits_branch = branch.to_owned();
                self.commits_source = if self.commits.is_empty() {
                    DataSource::Remote
                } else {
                    DataSource::Local
                };
                self.commits_error = None;
                if self.selected_commit.is_none() {
                    self.selected_commit = self.commits.first().map(|commit| commit.sha.clone());
                }
            }
            Err(error) => {
                self.toast(
                    format!("{}: {error}", self.translator.text(keys::ERROR_DB)),
                    ToastKind::Error,
                );
            }
        }
    }

    fn load_more_commits(&mut self, repo_id: i64, branch: &str) {
        let offset = self.commits.len() as u32;
        match self
            .db
            .list_commits_for_branch(repo_id, branch, 100, offset)
        {
            Ok(mut commits) => self.commits.append(&mut commits),
            Err(error) => {
                self.toast(
                    format!("{}: {error}", self.translator.text(keys::ERROR_DB)),
                    ToastKind::Error,
                );
            }
        }
    }

    fn load_charts(&mut self, repo_id: i64) {
        let branch = self
            .selected_repository()
            .map(|repository| repository.default_branch.clone())
            .unwrap_or_default();
        match self.db.list_all_commits_for_branch(repo_id, &branch) {
            Ok(commits) => {
                self.charts.commits = commits;
                self.charts.repo_id = Some(repo_id);
            }
            Err(error) => {
                self.toast(
                    format!("{}: {error}", self.translator.text(keys::ERROR_DB)),
                    ToastKind::Error,
                );
            }
        }
    }

    fn open_create(&mut self) {
        self.create_form = CreateForm::default();
        self.create_open = true;
    }

    fn open_repository(&mut self, repo_id: i64) {
        let Some(repository) = self
            .repos
            .iter()
            .find(|repository| repository.github_id == repo_id)
            .cloned()
        else {
            return;
        };
        self.selected_repo_id = Some(repo_id);
        self.screen = Screen::Repository;
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        self.repo_tab = RepoTab::Overview;
        self.repo_search.clear();
        self.commits.clear();
        self.commits_total = 0;
        self.commits_repo = None;
        self.commits_branch.clear();
        self.commits_source = DataSource::Remote;
        self.commits_pending = None;
        self.charts = ChartsState::default();
        self.selected_branch = repository.default_branch.clone();
        self.branches.clear();
        self.branches_repo = None;
        self.branches_pending = None;
        self.branches_source = DataSource::Remote;
        self.branches_error = None;
        self.code_path.clear();
        self.contents = None;
        self.contents_repo = None;
        self.contents_branch.clear();
        self.contents_source = DataSource::Remote;
        self.contents_pending = None;
        self.contents_error = None;
        self.readme = None;
        self.readme_repo = None;
        self.readme_branch.clear();
        self.readme_source = DataSource::Remote;
        self.readme_pending = None;
        self.readme_error = None;
        self.code_file = None;
        self.code_file_path = None;
        self.file_source = DataSource::Remote;
        self.file_pending = None;
        self.file_error = None;
        self.releases.clear();
        self.releases_repo = None;
        self.releases_source = DataSource::Remote;
        self.releases_pending = None;
        self.releases_error = None;
        self.issues.clear();
        self.issues_repo = None;
        self.issues_state = "open".to_owned();
        self.issues_source = DataSource::Remote;
        self.issues_pending = None;
        self.issues_error = None;
        self.selected_issue = None;
        self.issue_detail_source = DataSource::Remote;
        self.issue_detail_pending = None;
        self.issue_detail_error = None;
        self.issue_comments.clear();
        self.issue_comments_source = DataSource::Remote;
        self.issue_comments_pending = None;
        self.issue_comments_error = None;
        self.pulls.clear();
        self.pulls_repo = None;
        self.pulls_state = "open".to_owned();
        self.pulls_source = DataSource::Remote;
        self.pulls_pending = None;
        self.pulls_error = None;
        self.selected_pull = None;
        self.pull_detail_source = DataSource::Remote;
        self.pull_detail_pending = None;
        self.pull_detail_error = None;
        self.pull_comments.clear();
        self.pull_comments_source = DataSource::Remote;
        self.pull_comments_pending = None;
        self.pull_comments_error = None;
        self.pull_files.clear();
        self.pull_files_source = DataSource::Remote;
        self.pull_files_pending = None;
        self.pull_files_error = None;
        self.pull_diff = None;
        self.pull_diff_state = DiffViewState::default();
        self.actions_filter = "all".to_owned();
        self.actions_runs.clear();
        self.actions_repo = None;
        self.actions_source = DataSource::Remote;
        self.actions_pending = None;
        self.actions_error = None;
        self.selected_run = None;
        self.action_jobs.clear();
        self.action_jobs_source = DataSource::Remote;
        self.action_jobs_pending = None;
        self.action_jobs_error = None;
        self.action_logs.clear();
        self.action_log_job = None;
        self.action_log_source = DataSource::Remote;
        self.action_log_pending = None;
        self.action_log_error = None;
        self.diff = None;
        self.diff_label.clear();
        self.diff_error = None;
        self.diff_loading = false;
        self.diff_pending = None;
        self.selected_commit = None;
        self.base_branch.clear();
        self.head_branch.clear();
        match self.db.list_branches(repo_id) {
            Ok(branches) if !branches.is_empty() => {
                self.branches = branches;
                self.branches_repo = Some(repo_id);
                self.branches_source = DataSource::Local;
                self.select_default_branches();
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "could not load cached branches"),
        }
        self.load_commits(repo_id, &self.selected_branch.clone());
    }

    fn selected_repository(&self) -> Option<&Repository> {
        let repo_id = self.selected_repo_id?;
        self.repos
            .iter()
            .find(|repository| repository.github_id == repo_id)
    }

    fn sync_all(&mut self) {
        if let Some(sync) = &self.sync {
            if let Err(error) = sync.sync_all() {
                self.toast(error.to_string(), ToastKind::Error);
            }
        } else if self.client.is_some() {
            self.start_sync();
            if let Some(sync) = &self.sync {
                let _ = sync.sync_all();
            }
        } else {
            self.toast(self.translator.text(keys::ERROR_AUTH), ToastKind::Error);
        }
    }

    fn sync_repository(&mut self, repo_id: i64) {
        if let Some(sync) = &self.sync {
            if let Err(error) = sync.sync_repo(repo_id) {
                self.toast(error.to_string(), ToastKind::Error);
            }
        } else if self.client.is_some() {
            self.start_sync();
            if let Some(sync) = &self.sync {
                let _ = sync.sync_repo(repo_id);
            }
        }
    }

    fn submit_create(&mut self) {
        let name = self.create_form.name.trim().to_owned();
        if let Err(error) = validate_repo_name(&name) {
            self.create_form.error = Some(self.validation_text(&error));
            return;
        }
        if let Err(error) = validate_description(Some(&self.create_form.description)) {
            self.create_form.error = Some(self.validation_text(&error));
            return;
        }
        let Some(client) = self.client.clone() else {
            self.create_form.error = Some(self.translator.text(keys::ERROR_AUTH));
            return;
        };
        let db = self.db.clone();
        let request = CreateRepoRequest {
            name,
            description: if self.create_form.description.trim().is_empty() {
                None
            } else {
                Some(self.create_form.description.trim().to_owned())
            },
            private: self.create_form.private,
            auto_init: self.create_form.auto_init,
            gitignore_template: if self.create_form.gitignore.is_empty() {
                None
            } else {
                Some(self.create_form.gitignore.clone())
            },
            license_template: if self.create_form.license.is_empty() {
                None
            } else {
                Some(self.create_form.license.clone())
            },
        };
        self.create_form.error = None;
        self.create_pending = Some(self.jobs.spawn(async move {
            let repository = client.create_repository(&request).await?;
            if let Err(error) = db.upsert_repositories(std::slice::from_ref(&repository)) {
                tracing::warn!(%error, "could not cache created repository");
            }
            Ok(repository)
        }));
    }

    fn ensure_branches(&mut self, repository: &Repository) {
        if self.branches_repo == Some(repository.github_id) {
            if !self.branches.is_empty() {
                self.branches_source = DataSource::Local;
            }
            return;
        }
        if self.branches_pending.is_some() {
            return;
        }
        if let Ok(branches) = self.db.list_branches(repository.github_id)
            && !branches.is_empty()
        {
            self.branches = branches;
            self.branches_repo = Some(repository.github_id);
            self.branches_source = DataSource::Local;
            self.branches_error = None;
            self.select_default_branches();
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.branches_repo = Some(repository.github_id);
        self.branches_source = DataSource::Loading;
        self.branches_error = None;
        let db = self.db.clone();
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let repo_id = repository.github_id;
        self.branches_pending = Some(self.jobs.spawn(async move {
            let branches = client.list_branches(&owner, &name).await?;
            if let Err(error) = db.replace_branches(repo_id, &branches) {
                tracing::warn!(%error, "could not cache branches");
            }
            Ok(branches)
        }));
    }

    fn select_default_branches(&mut self) {
        let Some(repository) = self.selected_repository() else {
            return;
        };
        let default = repository.default_branch.clone();
        if self.selected_branch.is_empty()
            || !self
                .branches
                .iter()
                .any(|branch| branch.name == self.selected_branch)
        {
            self.selected_branch = default.clone();
        }
        if self.base_branch.is_empty()
            || !self
                .branches
                .iter()
                .any(|branch| branch.name == self.base_branch)
        {
            self.base_branch = default.clone();
        }
        if self.head_branch.is_empty()
            || !self
                .branches
                .iter()
                .any(|branch| branch.name == self.head_branch)
            || self.head_branch == self.base_branch
        {
            self.head_branch = self
                .branches
                .iter()
                .find(|branch| branch.name != self.base_branch)
                .map(|branch| branch.name.clone())
                .unwrap_or(default);
        }
    }

    fn switch_branch(&mut self, repository: &Repository, branch: &str) {
        if self.selected_branch == branch {
            return;
        }
        self.selected_branch = branch.to_owned();
        self.selected_commit = None;
        self.commits.clear();
        self.commits_repo = None;
        self.commits_branch.clear();
        self.commits_source = DataSource::Remote;
        self.commits_error = None;
        self.contents = None;
        self.contents_repo = None;
        self.contents_branch.clear();
        self.contents_source = DataSource::Remote;
        self.contents_error = None;
        self.readme = None;
        self.readme_repo = None;
        self.readme_branch.clear();
        self.readme_source = DataSource::Remote;
        self.readme_error = None;
        self.code_file = None;
        self.code_file_path = None;
        self.file_source = DataSource::Remote;
        self.file_error = None;
        self.code_path.clear();
        self.load_commits(repository.github_id, branch);
    }

    fn ensure_commits(&mut self, repository: &Repository) {
        let branch = if self.selected_branch.is_empty() {
            repository.default_branch.clone()
        } else {
            self.selected_branch.clone()
        };
        if self.commits_repo == Some(repository.github_id)
            && self.commits_branch == branch
            && self.commits_source == DataSource::Local
        {
            return;
        }
        if self.commits_pending.is_some()
            && self.commits_pending_repo == Some(repository.github_id)
            && self.commits_pending_branch.as_deref() == Some(branch.as_str())
        {
            return;
        }
        if self.commits_error.is_some()
            && self.commits_repo == Some(repository.github_id)
            && self.commits_branch == branch
        {
            return;
        }
        match self
            .db
            .list_commits_for_branch(repository.github_id, &branch, 100, 0)
        {
            Ok(commits) if !commits.is_empty() => {
                self.commits = commits;
                self.commits_total = self
                    .db
                    .count_commits_for_branch(repository.github_id, &branch)
                    .unwrap_or(0);
                self.commits_repo = Some(repository.github_id);
                self.commits_branch = branch;
                self.commits_source = DataSource::Local;
                self.commits_error = None;
                if self.selected_commit.is_none() {
                    self.selected_commit = self.commits.first().map(|commit| commit.sha.clone());
                }
                return;
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "could not load cached branch commits"),
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.commits_source = DataSource::Loading;
        self.commits_pending_repo = Some(repository.github_id);
        self.commits_pending_branch = Some(branch.clone());
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let repo_id = repository.github_id;
        self.commits_pending = Some(self.jobs.spawn(async move {
            let since = DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_else(Utc::now);
            let mut cursor = None;
            let mut commits = Vec::new();
            for _ in 0..5 {
                let page = client
                    .fetch_commits_page(&owner, &name, &branch, since, cursor.as_deref())
                    .await?;
                commits.extend(page.commits);
                if !page.has_next {
                    break;
                }
                cursor = page.end_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            Ok(BranchCommitsJob {
                repo_id,
                branch,
                commits,
            })
        }));
    }

    fn ensure_code(&mut self, repository: &Repository) {
        self.ensure_branches(repository);
        let branch = if self.selected_branch.is_empty() {
            repository.default_branch.clone()
        } else {
            self.selected_branch.clone()
        };
        if (self.contents_repo != Some(repository.github_id)
            || self.contents_branch != branch
            || self.contents.is_none())
            && self.contents_pending.is_none()
        {
            self.load_contents(repository, &branch, &self.code_path.clone(), false);
        }
        if (self.readme_repo != Some(repository.github_id) || self.readme_branch != branch)
            && self.readme_pending.is_none()
        {
            self.load_readme(repository, &branch, false);
        }
    }

    fn load_contents(&mut self, repository: &Repository, branch: &str, path: &str, refresh: bool) {
        if self.contents_pending.is_some() {
            return;
        }
        if !refresh
            && self.contents_error.is_some()
            && self.contents_repo == Some(repository.github_id)
            && self.contents_branch == branch
            && self.code_path == path
        {
            return;
        }
        self.code_path = path.to_owned();
        self.code_file = None;
        self.code_file_path = None;
        self.file_source = DataSource::Remote;
        self.file_error = None;
        if !refresh
            && let Some(payload) = self
                .db
                .get_content_cache(repository.github_id, branch, path)
                .ok()
                .flatten()
                .and_then(|value| serde_json::from_str::<ContentPayload>(&value).ok())
        {
            self.contents = Some(payload);
            self.contents_repo = Some(repository.github_id);
            self.contents_branch = branch.to_owned();
            self.contents_source = DataSource::Local;
            self.contents_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.contents_repo = Some(repository.github_id);
        self.contents_branch = branch.to_owned();
        self.contents_source = DataSource::Loading;
        self.contents_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let path = path.to_owned();
        let branch = branch.to_owned();
        self.contents_pending = Some(
            self.jobs
                .spawn(async move { client.get_contents(&owner, &name, &path, &branch).await }),
        );
    }

    fn load_readme(&mut self, repository: &Repository, branch: &str, refresh: bool) {
        if self.readme_pending.is_some() {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_content_cache(repository.github_id, branch, "__readme__")
                .ok()
                .flatten()
        {
            self.readme = serde_json::from_str::<ContentPayload>(&cached).ok();
            self.readme_repo = Some(repository.github_id);
            self.readme_branch = branch.to_owned();
            self.readme_source = DataSource::Local;
            self.readme_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.readme_repo = Some(repository.github_id);
        self.readme_branch = branch.to_owned();
        self.readme_source = DataSource::Loading;
        self.readme_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let branch = branch.to_owned();
        self.readme_pending = Some(
            self.jobs
                .spawn(async move { client.get_readme(&owner, &name, &branch).await }),
        );
    }

    fn load_file(&mut self, repository: &Repository, branch: &str, path: &str, refresh: bool) {
        if self.file_pending.is_some() {
            return;
        }
        self.code_file_path = Some(path.to_owned());
        self.code_file = None;
        if !refresh
            && let Some(payload) = self
                .db
                .get_content_cache(repository.github_id, branch, path)
                .ok()
                .flatten()
                .and_then(|value| serde_json::from_str::<ContentPayload>(&value).ok())
        {
            self.code_file = Some(payload);
            self.file_source = DataSource::Local;
            self.file_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.file_source = DataSource::Loading;
        self.file_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let path = path.to_owned();
        let branch = branch.to_owned();
        self.file_pending = Some(
            self.jobs
                .spawn(async move { client.get_contents(&owner, &name, &path, &branch).await }),
        );
    }

    fn load_releases(&mut self, repository: &Repository, refresh: bool) {
        if self.releases_pending.is_some() {
            return;
        }
        if !refresh && self.releases_repo == Some(repository.github_id) {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_release_cache(repository.github_id)
                .ok()
                .flatten()
            && let Ok(releases) = serde_json::from_str::<Vec<Release>>(&cached)
        {
            self.releases = releases;
            self.releases_repo = Some(repository.github_id);
            self.releases_source = DataSource::Local;
            self.releases_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.releases_repo = Some(repository.github_id);
        self.releases_source = DataSource::Loading;
        self.releases_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        self.releases_pending = Some(
            self.jobs
                .spawn(async move { client.list_releases(&owner, &name).await }),
        );
    }

    fn load_issues(&mut self, repository: &Repository, state: &str, refresh: bool) {
        if self.issues_pending.is_some() {
            return;
        }
        if !refresh && self.issues_repo == Some(repository.github_id) && self.issues_state == state
        {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(repository.github_id, "issues", state)
                .ok()
                .flatten()
            && let Ok(issues) = serde_json::from_str::<Vec<Issue>>(&cached)
        {
            self.issues = issues;
            self.issues_repo = Some(repository.github_id);
            self.issues_state = state.to_owned();
            self.issues_source = DataSource::Local;
            self.issues_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.issues_repo = Some(repository.github_id);
        self.issues_state = state.to_owned();
        self.issues_source = DataSource::Loading;
        self.issues_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let state = state.to_owned();
        self.issues_pending = Some(
            self.jobs
                .spawn(async move { client.list_issues(&owner, &name, &state).await }),
        );
    }

    fn select_issue(&mut self, repository: &Repository, issue: Issue) {
        self.selected_issue = Some(issue.clone());
        self.issue_detail_source = DataSource::Remote;
        self.issue_detail_pending = None;
        self.issue_detail_error = None;
        self.issue_comments.clear();
        self.issue_comments_source = DataSource::Remote;
        self.issue_comments_pending = None;
        self.issue_comments_error = None;
        self.load_issue_detail(repository, &issue, false);
        self.load_issue_comments(repository, &issue, false);
    }

    fn load_issue_detail(&mut self, repository: &Repository, issue: &Issue, refresh: bool) {
        if self.issue_detail_pending.is_some() {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(repository.github_id, "issue", &issue.number.to_string())
                .ok()
                .flatten()
            && let Ok(detail) = serde_json::from_str::<Issue>(&cached)
        {
            self.selected_issue = Some(detail);
            self.issue_detail_source = DataSource::Local;
            self.issue_detail_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.issue_detail_source = DataSource::Loading;
        self.issue_detail_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let number = issue.number;
        self.issue_detail_pending = Some(
            self.jobs
                .spawn(async move { client.get_issue(&owner, &name, number).await }),
        );
    }

    fn load_issue_comments(&mut self, repository: &Repository, issue: &Issue, refresh: bool) {
        if self.issue_comments_pending.is_some() {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(
                    repository.github_id,
                    "issue-comments",
                    &issue.number.to_string(),
                )
                .ok()
                .flatten()
            && let Ok(comments) = serde_json::from_str::<Vec<IssueComment>>(&cached)
        {
            self.issue_comments = comments;
            self.issue_comments_source = DataSource::Local;
            self.issue_comments_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.issue_comments_source = DataSource::Loading;
        self.issue_comments_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let number = issue.number;
        self.issue_comments_pending = Some(
            self.jobs
                .spawn(async move { client.list_issue_comments(&owner, &name, number).await }),
        );
    }

    fn load_pulls(&mut self, repository: &Repository, state: &str, refresh: bool) {
        if self.pulls_pending.is_some() {
            return;
        }
        if !refresh && self.pulls_repo == Some(repository.github_id) && self.pulls_state == state {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(repository.github_id, "pulls", state)
                .ok()
                .flatten()
            && let Ok(pulls) = serde_json::from_str::<Vec<PullRequest>>(&cached)
        {
            self.pulls = pulls;
            self.pulls_repo = Some(repository.github_id);
            self.pulls_state = state.to_owned();
            self.pulls_source = DataSource::Local;
            self.pulls_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.pulls_repo = Some(repository.github_id);
        self.pulls_state = state.to_owned();
        self.pulls_source = DataSource::Loading;
        self.pulls_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let state = state.to_owned();
        self.pulls_pending = Some(
            self.jobs
                .spawn(async move { client.list_pulls(&owner, &name, &state).await }),
        );
    }

    fn select_pull(&mut self, repository: &Repository, pull: PullRequest) {
        self.selected_pull = Some(pull.clone());
        self.pull_detail_source = DataSource::Remote;
        self.pull_detail_pending = None;
        self.pull_detail_error = None;
        self.pull_comments.clear();
        self.pull_comments_source = DataSource::Remote;
        self.pull_comments_pending = None;
        self.pull_comments_error = None;
        self.pull_files.clear();
        self.pull_files_source = DataSource::Remote;
        self.pull_files_pending = None;
        self.pull_files_error = None;
        self.pull_diff = None;
        self.pull_diff_state = DiffViewState::default();
        self.load_pull_detail(repository, &pull, false);
        self.load_pull_comments(repository, &pull, false);
    }

    fn load_pull_detail(&mut self, repository: &Repository, pull: &PullRequest, refresh: bool) {
        if self.pull_detail_pending.is_some() {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(repository.github_id, "pull", &pull.number.to_string())
                .ok()
                .flatten()
            && let Ok(detail) = serde_json::from_str::<PullRequest>(&cached)
        {
            self.selected_pull = Some(detail);
            self.pull_detail_source = DataSource::Local;
            self.pull_detail_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.pull_detail_source = DataSource::Loading;
        self.pull_detail_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let number = pull.number;
        self.pull_detail_pending = Some(
            self.jobs
                .spawn(async move { client.get_pull(&owner, &name, number).await }),
        );
    }

    fn load_pull_comments(&mut self, repository: &Repository, pull: &PullRequest, refresh: bool) {
        if self.pull_comments_pending.is_some() {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(
                    repository.github_id,
                    "pull-comments",
                    &pull.number.to_string(),
                )
                .ok()
                .flatten()
            && let Ok(comments) = serde_json::from_str::<Vec<IssueComment>>(&cached)
        {
            self.pull_comments = comments;
            self.pull_comments_source = DataSource::Local;
            self.pull_comments_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.pull_comments_source = DataSource::Loading;
        self.pull_comments_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let number = pull.number;
        self.pull_comments_pending = Some(
            self.jobs
                .spawn(async move { client.list_issue_comments(&owner, &name, number).await }),
        );
    }

    fn load_pull_files(&mut self, repository: &Repository, pull: &PullRequest, refresh: bool) {
        if self.pull_files_pending.is_some() {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(repository.github_id, "pull-files", &pull.number.to_string())
                .ok()
                .flatten()
            && let Ok(files) = serde_json::from_str::<Vec<FilePatch>>(&cached)
        {
            self.pull_files = files;
            self.pull_diff = Some(Diff::from_patches(&self.pull_files));
            self.pull_files_source = DataSource::Local;
            self.pull_files_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.pull_files_source = DataSource::Loading;
        self.pull_files_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let number = pull.number;
        self.pull_files_pending = Some(
            self.jobs
                .spawn(async move { client.list_pull_files(&owner, &name, number).await }),
        );
    }

    fn load_actions(&mut self, repository: &Repository, refresh: bool) {
        if self.actions_pending.is_some() {
            return;
        }
        if !refresh && self.actions_repo == Some(repository.github_id) {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(repository.github_id, "actions-runs", "all")
                .ok()
                .flatten()
            && let Ok(runs) = serde_json::from_str::<Vec<WorkflowRun>>(&cached)
        {
            self.actions_runs = runs;
            self.actions_repo = Some(repository.github_id);
            self.actions_source = DataSource::Local;
            self.actions_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.actions_repo = Some(repository.github_id);
        self.actions_source = DataSource::Loading;
        self.actions_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        self.actions_pending = Some(
            self.jobs
                .spawn(async move { client.list_workflow_runs(&owner, &name).await }),
        );
    }

    fn select_run(&mut self, repository: &Repository, run: WorkflowRun) {
        self.selected_run = Some(run.clone());
        self.action_jobs.clear();
        self.action_jobs_source = DataSource::Remote;
        self.action_jobs_pending = None;
        self.action_jobs_error = None;
        self.action_logs.clear();
        self.action_log_job = None;
        self.action_log_source = DataSource::Remote;
        self.action_log_pending = None;
        self.action_log_error = None;
        self.load_run_jobs(repository, &run, false);
    }

    fn load_run_jobs(&mut self, repository: &Repository, run: &WorkflowRun, refresh: bool) {
        if self.action_jobs_pending.is_some() {
            return;
        }
        if !refresh
            && let Some(cached) = self
                .db
                .get_api_cache(repository.github_id, "actions-jobs", &run.id.to_string())
                .ok()
                .flatten()
            && let Ok(jobs) = serde_json::from_str::<Vec<WorkflowJob>>(&cached)
        {
            self.action_jobs = jobs;
            self.action_jobs_source = DataSource::Local;
            self.action_jobs_error = None;
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.action_jobs_source = DataSource::Loading;
        self.action_jobs_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let run_id = run.id;
        self.action_jobs_pending = Some(
            self.jobs
                .spawn(async move { client.list_workflow_jobs(&owner, &name, run_id).await }),
        );
    }

    fn load_job_logs(&mut self, repository: &Repository, job: &WorkflowJob) {
        self.action_log_job = Some(job.id);
        if self.action_logs.contains_key(&job.id) {
            self.action_log_source = DataSource::Local;
            self.action_log_error = None;
            return;
        }
        if self.action_log_pending.is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.action_log_source = DataSource::Loading;
        self.action_log_error = None;
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        let job_id = job.id;
        self.action_log_pending = Some(self.jobs.spawn(async move {
            client
                .get_job_logs(&owner, &name, job_id)
                .await
                .map(|log| (job_id, log))
        }));
    }

    fn start_diff(&mut self) {
        let Some(repository) = self.selected_repository().cloned() else {
            return;
        };
        let Some(client) = self.client.clone() else {
            self.diff_error = Some(self.translator.text(keys::ERROR_AUTH));
            return;
        };
        let source = self.diff_source;
        let (base_key, head_key, base_sha, head_sha, label) = match source {
            DiffSource::Commit => {
                let Some(sha) = self.selected_commit.clone() else {
                    self.diff_error = Some(self.translator.text(keys::DIFF_SELECT_COMMIT));
                    return;
                };
                (
                    sha.clone(),
                    "commit".to_owned(),
                    sha.clone(),
                    String::new(),
                    translator_commit_label(&self.translator, &sha),
                )
            }
            DiffSource::Compare => {
                let Some(base) = self
                    .branches
                    .iter()
                    .find(|branch| branch.name == self.base_branch)
                    .cloned()
                else {
                    self.diff_error = Some(self.translator.text(keys::DIFF_SELECT_BASE));
                    return;
                };
                let Some(head) = self
                    .branches
                    .iter()
                    .find(|branch| branch.name == self.head_branch)
                    .cloned()
                else {
                    self.diff_error = Some(self.translator.text(keys::DIFF_SELECT_HEAD));
                    return;
                };
                (
                    base.sha.clone(),
                    head.sha.clone(),
                    base.sha,
                    head.sha,
                    format!("{} → {}", base.name, head.name),
                )
            }
        };
        let repo_id = repository.github_id;
        let db = self.db.clone();
        let owner = repository.owner.clone();
        let name = repository.name.clone();
        self.diff_loading = true;
        self.diff_error = None;
        self.diff = None;
        self.diff_label = label.clone();
        let job_label = label;
        let cache_enabled = self.cache_enabled();
        self.diff_pending = Some(self.jobs.spawn(async move {
            if let Some(payload) = db
                .get_diff_cache(repo_id, &base_key, &head_key)
                .ok()
                .flatten()
                && let Ok(files) = serde_json::from_str::<Vec<FilePatch>>(&payload)
            {
                return Ok(DiffLoad {
                    files,
                    label: job_label,
                });
            }
            let files = match source {
                DiffSource::Commit => client.commit_diff(&owner, &name, &base_sha).await?.files,
                DiffSource::Compare => {
                    client
                        .compare(&owner, &name, &base_sha, &head_sha)
                        .await?
                        .files
                }
            };
            if let Ok(payload) = serde_json::to_string(&files)
                && cache_enabled
                && payload.len() <= 4_000_000
                && let Err(error) = db.put_diff_cache(repo_id, &base_key, &head_key, &payload)
            {
                tracing::warn!(%error, "could not cache diff");
            }
            Ok(DiffLoad {
                files,
                label: job_label,
            })
        }));
    }

    fn open_commit_diff(&mut self, sha: String) {
        self.repo_tab = RepoTab::Diff;
        self.diff_source = DiffSource::Commit;
        self.selected_commit = Some(sha);
        self.diff = None;
        self.diff_error = None;
        self.start_diff();
    }

    fn save_settings(&mut self, form: SettingsForm) {
        let language_changed = self.config.settings.ui.language != form.language;
        let sync_changed = self.config.settings.sync.interval_minutes != form.interval_minutes
            || self.config.settings.sync.backfill_days != form.backfill_days
            || self.config.settings.sync.sync_all_accessible != form.sync_all;
        let cache_changed = self.config.settings.cache.enabled != form.cache_enabled
            || self.config.settings.cache.keep_days != form.cache_keep_days
            || self.config.settings.cache.max_file_kb != form.cache_max_file_kb;
        self.settings = form.clone();
        self.config.settings.ui.theme = form.theme;
        self.config.settings.ui.language = form.language.clone();
        self.config.settings.sync.interval_minutes = form.interval_minutes;
        self.config.settings.sync.backfill_days = form.backfill_days;
        self.config.settings.sync.sync_all_accessible = form.sync_all;
        self.config.settings.cache.enabled = form.cache_enabled;
        self.config.settings.cache.keep_days = form.cache_keep_days;
        self.config.settings.cache.max_file_kb = form.cache_max_file_kb;
        self.config.settings.normalize();
        if let Err(error) = self.config.save() {
            self.toast(
                format!("{}: {error}", self.translator.text(keys::ERROR_CONFIG)),
                ToastKind::Error,
            );
        }
        if cache_changed
            && let Err(error) = self.db.prune_cache(self.config.settings.cache.keep_days)
        {
            tracing::warn!(%error, "could not prune cache after settings change");
        }
        if language_changed {
            match Translator::new(&self.config.settings.ui.language) {
                Ok(translator) => self.translator = translator,
                Err(error) => self.toast(
                    format!("{}: {error}", self.translator.text(keys::ERROR_I18N)),
                    ToastKind::Error,
                ),
            }
        }
        if sync_changed {
            self.start_sync();
        }
        self.toast(
            self.translator.text(keys::SETTINGS_SAVED),
            ToastKind::Success,
        );
    }

    fn refresh_theme(&mut self, ctx: &egui::Context) {
        let system_dark = ctx.system_theme().map(|theme| theme == egui::Theme::Dark);
        let theme = Theme::resolve(self.config.settings.ui.theme, system_dark);
        if theme.dark != self.theme.dark || theme.mode != self.theme.mode {
            self.theme = theme;
            self.theme.apply(ctx);
            if self.diff.is_some() {
                self.highlighter = Some(Highlighter::new(theme.dark));
            }
        }
    }

    fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.config.settings.ui.theme = if self.theme.dark {
            ThemePreference::Light
        } else {
            ThemePreference::Dark
        };
        self.settings.theme = self.config.settings.ui.theme;
        let _ = self.config.save();
        self.refresh_theme(ctx);
    }

    fn reload_auth_action(&mut self) {
        self.reload_auth();
        self.reload_repositories();
    }

    fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toasts.push(Toast {
            text: text.into(),
            kind,
            created: Instant::now(),
        });
    }

    fn cache_enabled(&self) -> bool {
        self.config.settings.cache.enabled
    }

    fn cache_api(&self, repo_id: i64, kind: &str, key: &str, payload: &str) {
        if !self.cache_enabled() || payload.len() > 512 * 1024 {
            return;
        }
        if let Err(error) = self.db.put_api_cache(repo_id, kind, key, payload) {
            tracing::warn!(%error, "could not write API cache");
        }
    }

    fn cache_content(&self, repo_id: i64, branch: &str, path: &str, payload: &str) {
        let limit = self.config.settings.cache.max_file_kb as usize * 1024;
        if !self.cache_enabled() || payload.len() > limit {
            return;
        }
        if let Err(error) = self.db.put_content_cache(repo_id, branch, path, payload) {
            tracing::warn!(%error, "could not write content cache");
        }
    }

    fn cache_release(&self, repo_id: i64, payload: &str) {
        if !self.cache_enabled() || payload.len() > 2 * 1024 * 1024 {
            return;
        }
        if let Err(error) = self.db.put_release_cache(repo_id, payload) {
            tracing::warn!(%error, "could not write release cache");
        }
    }

    fn validation_text(&self, error: &ValidationError) -> String {
        self.translator.text(error.code())
    }

    fn github_error_title(&self, error: &GithubError) -> String {
        match error {
            GithubError::GhMissing => self.translator.text(keys::ERROR_GH_MISSING),
            GithubError::GhCommand(_) | GithubError::GhNotLoggedIn { .. } => {
                self.translator.text(keys::ERROR_GH_NOT_LOGGED_IN)
            }
            GithubError::Network(_) => self.translator.text(keys::ERROR_NETWORK),
            GithubError::AuthInvalid { .. } => self.translator.text(keys::ERROR_AUTH),
            GithubError::Forbidden { .. } => self.translator.text(keys::ERROR_FORBIDDEN),
            GithubError::NotFound { .. } => self.translator.text(keys::ERROR_NOT_FOUND),
            GithubError::RateLimited { reset_at, .. } => {
                let time = relative_time(&self.translator, *reset_at, Utc::now());
                self.translator
                    .text_args(keys::ERROR_RATE_LIMIT, &[Arg::str("time", &time)])
            }
            GithubError::Validation { .. } => self.translator.text(keys::ERROR_VALIDATION),
            GithubError::GraphQl { .. } => self.translator.text(keys::ERROR_GRAPHQL),
            GithubError::Decode { .. } => self.translator.text(keys::ERROR_DECODE),
            GithubError::Http { .. } => self.translator.text(keys::ERROR_GENERIC),
            GithubError::ScopeMissing { scope } => self
                .translator
                .text_args(keys::ERROR_SCOPE_MISSING, &[Arg::str("scope", scope)]),
        }
    }

    fn auth_banner(&self, ui: &mut Ui) -> bool {
        let mut reload = false;
        if let Some(error) = &self.auth_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_AUTH),
                error,
                self.theme.palette,
            );
            ui.horizontal(|ui| {
                if ui
                    .add(secondary_button(
                        self.translator.text(keys::COMMON_RELOAD_TOKEN),
                        self.theme.palette,
                    ))
                    .clicked()
                {
                    reload = true;
                }
                muted(ui, "gh auth login", self.theme.palette);
            });
        }
        reload
    }

    fn filtered_repositories(&self) -> Vec<Repository> {
        let login = self.account.as_ref().map(|account| account.login.as_str());
        let query = self.repo_search.trim().to_ascii_lowercase();
        self.repos
            .iter()
            .filter(|repository| {
                if !query.is_empty() {
                    let haystack = format!(
                        "{} {} {} {}",
                        repository.full_name,
                        repository.description.as_deref().unwrap_or_default(),
                        repository.language.as_deref().unwrap_or_default(),
                        repository.owner
                    )
                    .to_ascii_lowercase();
                    if !haystack.contains(&query) {
                        return false;
                    }
                }
                match self.repo_filter {
                    RepoFilter::All => true,
                    RepoFilter::Own => login.is_some_and(|login| repository.owner == login),
                    RepoFilter::Others => login.is_none_or(|login| repository.owner != login),
                }
            })
            .cloned()
            .collect()
    }

    fn screen_repositories(&mut self, ui: &mut Ui) {
        let palette = self.theme.palette;
        let mut create = false;
        let mut sync_all = false;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::REPOS_TITLE))
                        .size(24.0)
                        .strong()
                        .color(palette.fg_default),
                );
                ui.label(
                    RichText::new(self.translator.text(keys::REPOS_SUBTITLE))
                        .size(13.0)
                        .color(palette.fg_muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(primary_button(
                        self.translator.text(keys::REPOS_CREATE),
                        palette,
                    ))
                    .clicked()
                {
                    create = true;
                }
                if ui
                    .add(secondary_button(
                        self.translator.text(keys::REPOS_SYNC_ALL),
                        palette,
                    ))
                    .clicked()
                {
                    sync_all = true;
                }
            });
        });
        ui.add_space(14.0);
        let mut repo_view = self.repo_view;
        ui.horizontal(|ui| {
            search_input(
                ui,
                &mut self.repo_search,
                &self.translator.text(keys::REPOS_SEARCH_PLACEHOLDER),
                palette,
            );
            ui.selectable_value(
                &mut self.repo_filter,
                RepoFilter::All,
                self.translator.text(keys::REPOS_FILTER_ALL),
            );
            ui.selectable_value(
                &mut self.repo_filter,
                RepoFilter::Own,
                self.translator.text(keys::REPOS_FILTER_OWN),
            );
            ui.selectable_value(
                &mut self.repo_filter,
                RepoFilter::Others,
                self.translator.text(keys::REPOS_FILTER_OTHERS),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let count = self.filtered_repositories().len();
                ui.label(
                    RichText::new(
                        self.translator
                            .text_args(keys::REPOS_COUNT, &[Arg::int("count", count as i64)]),
                    )
                    .size(12.0)
                    .color(palette.fg_muted),
                );
                ui.selectable_value(
                    &mut repo_view,
                    RepoView::Table,
                    self.translator.text(keys::REPOS_VIEW_TABLE),
                );
                ui.selectable_value(
                    &mut repo_view,
                    RepoView::Grid,
                    self.translator.text(keys::REPOS_VIEW_GRID),
                );
            });
        });
        self.repo_view = repo_view;
        ui.add_space(10.0);

        if self.auth_error.is_some() {
            if self.auth_banner(ui) {
                self.reload_auth_action();
            }
            ui.add_space(8.0);
        }
        if self.sync_state.running {
            let progress = if self.sync_state.total == 0 {
                0.0
            } else {
                self.sync_state.current as f32 / self.sync_state.total as f32
            };
            let text = if let Some(name) = &self.sync_state.current_repo_name {
                format!(
                    "{name}  ·  {}/{}",
                    self.sync_state.current, self.sync_state.total
                )
            } else {
                self.translator.text(keys::SYNC_STARTING)
            };
            ui.add(egui::ProgressBar::new(progress).text(text));
            ui.add_space(8.0);
        }
        if let Some(until) = self.sync_state.paused_until {
            let time = relative_time(&self.translator, until, Utc::now());
            info_banner(
                ui,
                &self
                    .translator
                    .text_args(keys::SYNC_RATE_LIMITED_UNTIL, &[Arg::str("time", &time)]),
                palette,
            );
            ui.add_space(8.0);
        }
        if let Some(error) = &self.sync_state.error {
            error_banner(ui, &self.translator.text(keys::ERROR_SYNC), error, palette);
            ui.add_space(8.0);
        }

        let filtered = self.filtered_repositories();
        if filtered.is_empty() {
            empty_state(
                ui,
                &self.translator.text(keys::REPOS_EMPTY_TITLE),
                &self.translator.text(keys::REPOS_EMPTY_BODY),
                palette,
                Some(Icon::Repo),
            );
        } else {
            let clicked = match self.repo_view {
                RepoView::Grid => repo_grid(
                    ui,
                    &filtered,
                    &self.translator,
                    palette,
                    self.selected_repo_id,
                    self.sync_state.current_repo_id,
                ),
                RepoView::Table => repo_table(
                    ui,
                    &filtered,
                    &self.translator,
                    palette,
                    self.selected_repo_id,
                    self.sync_state.current_repo_id,
                ),
            };
            if let Some(repo_id) = clicked {
                self.open_repository(repo_id);
            }
        }

        if create {
            self.open_create();
        }
        if sync_all {
            self.sync_all();
        }
    }

    fn screen_repository(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let Some(repository) = self.selected_repository().cloned() else {
            self.screen = Screen::Repositories;
            return;
        };
        let palette = self.theme.palette;
        let mut back = false;
        let mut open_github = false;
        let mut sync_repo = false;
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(self.translator.text(keys::REPOS_DETAIL_BACK))
                            .size(12.0)
                            .color(palette.accent_fg),
                    )
                    .frame(false),
                )
                .clicked()
            {
                back = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(secondary_button(
                        format!("{} ↗", self.translator.text(keys::COMMON_OPEN_GITHUB)),
                        palette,
                    ))
                    .clicked()
                {
                    open_github = true;
                }
                if ui
                    .add(secondary_button(
                        self.translator.text(keys::COMMON_SYNC_NOW),
                        palette,
                    ))
                    .clicked()
                {
                    sync_repo = true;
                }
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            icon(ui, Icon::Repo, 24.0, palette.fg_muted);
            ui.label(
                RichText::new(&repository.owner)
                    .size(20.0)
                    .color(palette.fg_muted),
            );
            ui.label(RichText::new("/").size(20.0).color(palette.fg_subtle));
            ui.label(
                RichText::new(&repository.name)
                    .size(20.0)
                    .strong()
                    .color(palette.fg_default),
            );
            if repository.private {
                badge(
                    ui,
                    self.translator.text(keys::COMMON_PRIVATE),
                    palette.canvas_inset,
                    palette.fg_muted,
                );
            } else {
                outline_badge(ui, self.translator.text(keys::COMMON_PUBLIC), palette);
            }
            if repository.archived {
                badge(
                    ui,
                    self.translator.text(keys::REPOS_DETAIL_ARCHIVED),
                    palette.diff_del_bg,
                    palette.attention_fg,
                );
            }
        });
        if let Some(description) = &repository.description {
            ui.label(
                RichText::new(description)
                    .size(13.0)
                    .color(palette.fg_muted),
            );
        } else {
            ui.label(
                RichText::new(self.translator.text(keys::REPOS_DETAIL_NO_DESCRIPTION))
                    .size(13.0)
                    .color(palette.fg_subtle),
            );
        }
        ui.add_space(10.0);

        let tab_labels = [
            self.translator.text(keys::TAB_OVERVIEW),
            self.translator.text(keys::TAB_CODE),
            self.translator.text(keys::TAB_COMMITS),
            self.translator.text(keys::TAB_BRANCHES),
            self.translator.text(keys::TAB_RELEASES),
            self.translator.text(keys::TAB_ISSUES),
            self.translator.text(keys::TAB_PULLS),
            self.translator.text(keys::TAB_ACTIONS),
            self.translator.text(keys::TAB_CHARTS),
            self.translator.text(keys::TAB_DIFF),
        ];
        let tab_refs: Vec<&str> = tab_labels.iter().map(String::as_str).collect();
        let mut selected = self.repo_tab.index();
        if tabs(ui, &tab_refs, &mut selected, palette) {
            let tab = RepoTab::from_index(selected);
            self.repo_tab = tab;
            match tab {
                RepoTab::Code => self.ensure_code(&repository),
                RepoTab::Commits => self.ensure_commits(&repository),
                RepoTab::Branches => self.ensure_branches(&repository),
                RepoTab::Releases => self.load_releases(&repository, false),
                RepoTab::Issues => self.load_issues(&repository, &self.issues_state.clone(), false),
                RepoTab::Pulls => self.load_pulls(&repository, &self.pulls_state.clone(), false),
                RepoTab::Actions => self.load_actions(&repository, false),
                RepoTab::Charts => {
                    if self.charts.repo_id != Some(repository.github_id) {
                        self.load_charts(repository.github_id);
                    }
                }
                RepoTab::Diff => {
                    self.ensure_branches(&repository);
                    self.ensure_commits(&repository);
                    if self.selected_commit.is_none() {
                        self.selected_commit =
                            self.commits.first().map(|commit| commit.sha.clone());
                    }
                }
                RepoTab::Overview => {}
            }
        }

        match self.repo_tab {
            RepoTab::Overview => self.overview_panel(ui, &repository),
            RepoTab::Code => self.code_panel(ui, &repository),
            RepoTab::Commits => self.commits_panel(ui, &repository),
            RepoTab::Branches => self.branches_panel(ui, &repository),
            RepoTab::Releases => self.releases_panel(ui, &repository),
            RepoTab::Issues => self.issues_panel(ui, &repository),
            RepoTab::Pulls => self.pulls_panel(ui, &repository),
            RepoTab::Actions => self.actions_panel(ui, &repository),
            RepoTab::Charts => self.charts_panel(ui, &repository),
            RepoTab::Diff => self.diff_panel(ui, &repository),
        }

        if back {
            self.screen = Screen::Repositories;
            self.scroll_generation = self.scroll_generation.wrapping_add(1);
        }
        if open_github {
            ctx.open_url(egui::OpenUrl::new_tab(repository.html_url.clone()));
        }
        if sync_repo {
            self.sync_repository(repository.github_id);
        }
    }

    fn branch_combo(&mut self, ui: &mut Ui, repository: &Repository) -> bool {
        let previous = self.selected_branch.clone();
        if self.selected_branch.is_empty() {
            self.selected_branch = repository.default_branch.clone();
        }
        let branch_names: Vec<String> = self
            .branches
            .iter()
            .map(|branch| branch.name.clone())
            .collect();
        let default_branch = repository.default_branch.clone();
        egui::ComboBox::from_id_salt(("repo-branch", repository.github_id))
            .selected_text(self.selected_branch.clone())
            .width(220.0)
            .show_ui(ui, |ui| {
                if branch_names.is_empty() {
                    ui.selectable_value(
                        &mut self.selected_branch,
                        default_branch.clone(),
                        &default_branch,
                    );
                } else {
                    for branch in &branch_names {
                        ui.selectable_value(&mut self.selected_branch, branch.clone(), branch);
                    }
                }
            });
        if self.selected_branch != previous {
            self.switch_branch(repository, &self.selected_branch.clone());
            true
        } else {
            false
        }
    }

    fn code_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        self.ensure_code(repository);
        let mut refresh = false;
        let mut branch_changed = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::COMMON_BRANCH))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            branch_changed = self.branch_combo(ui, repository);
            source_badge(ui, self.contents_source, &self.translator, palette);
            if ui
                .add(secondary_button(
                    self.translator.text(keys::CODE_REFRESH),
                    palette,
                ))
                .clicked()
            {
                refresh = true;
            }
        });
        if branch_changed {
            self.ensure_code(repository);
        }
        if let Some(error) = &self.branches_error {
            error_banner(ui, &self.translator.text(keys::ERROR_SYNC), error, palette);
        }
        if refresh {
            self.load_contents(
                repository,
                &self.selected_branch.clone(),
                &self.code_path.clone(),
                true,
            );
            self.load_readme(repository, &self.selected_branch.clone(), true);
        }

        let mut navigate = None;
        if !self.code_path.is_empty() {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(self.translator.text(keys::CODE_ROOT))
                                .size(12.0)
                                .color(palette.accent_fg),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    navigate = Some(String::new());
                }
                let parts: Vec<String> = self
                    .code_path
                    .split('/')
                    .filter(|part| !part.is_empty())
                    .map(str::to_owned)
                    .collect();
                let mut accumulated = String::new();
                for part in parts {
                    if !accumulated.is_empty() {
                        ui.label(RichText::new("/").color(palette.fg_subtle));
                    }
                    accumulated.push_str(&part);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(&part).size(12.0).color(palette.accent_fg),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        navigate = Some(accumulated.clone());
                    }
                    accumulated.push('/');
                }
            });
            ui.add_space(4.0);
        }

        let mut open_file = None;
        if self.contents_pending.is_some() {
            loading(ui, &self.translator.text(keys::CODE_LOADING), palette);
        } else if let Some(error) = &self.contents_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
        } else if let Some(payload) = &self.contents {
            if payload.is_dir() {
                let mut entries = payload.entries.clone();
                entries.sort_by_key(|entry| (entry.kind != "dir", entry.name.to_ascii_lowercase()));
                if entries.is_empty() {
                    muted(ui, self.translator.text(keys::CODE_EMPTY), palette);
                } else {
                    ScrollArea::vertical()
                        .id_salt((
                            "code-directory",
                            repository.github_id,
                            self.code_path.clone(),
                        ))
                        .max_height(280.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let repo_id = repository.github_id;
                            let branch = self.selected_branch.clone();
                            for entry in entries {
                                let cached = self
                                    .db
                                    .get_content_cache(repo_id, &branch, &entry.path)
                                    .ok()
                                    .flatten()
                                    .is_some();
                                ui.horizontal(|ui| {
                                    icon(
                                        ui,
                                        if entry.kind == "dir" {
                                            Icon::Folder
                                        } else {
                                            Icon::File
                                        },
                                        15.0,
                                        palette.fg_muted,
                                    );
                                    let response = ui.add(
                                        egui::Button::new(
                                            RichText::new(&entry.name).size(12.0).color(
                                                if entry.kind == "dir" {
                                                    palette.accent_fg
                                                } else {
                                                    palette.fg_default
                                                },
                                            ),
                                        )
                                        .frame(false),
                                    );
                                    if response.clicked() {
                                        if entry.kind == "dir" {
                                            navigate = Some(entry.path.clone());
                                        } else {
                                            open_file = Some(entry.path.clone());
                                        }
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if let Some(url) = &entry.html_url
                                            && ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("↗")
                                                            .size(11.0)
                                                            .color(palette.fg_subtle),
                                                    )
                                                    .frame(false),
                                                )
                                                .clicked()
                                        {
                                            ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
                                        }
                                        icon(
                                            ui,
                                            if cached { Icon::Local } else { Icon::Cloud },
                                            11.0,
                                            if cached {
                                                palette.success_fg
                                            } else {
                                                palette.fg_subtle
                                            },
                                        );
                                        if entry.kind != "dir"
                                            && let Some(size) = entry.size
                                        {
                                            ui.label(
                                                RichText::new(format_bytes(size))
                                                    .size(10.5)
                                                    .color(palette.fg_subtle),
                                            );
                                        }
                                    });
                                });
                                ui.separator();
                            }
                        });
                }
            } else if let Some(content) = &payload.content {
                ui.add(
                    egui::Label::new(
                        RichText::new(content)
                            .monospace()
                            .size(11.5)
                            .color(palette.fg_default),
                    )
                    .wrap_mode(egui::TextWrapMode::Extend),
                );
            }
        }

        if let Some(path) = open_file {
            self.load_file(repository, &self.selected_branch.clone(), &path, false);
        }
        if let Some(path) = navigate {
            self.load_contents(repository, &self.selected_branch.clone(), &path, false);
        }

        self.code_file_view(ui, repository);

        if self.code_path.is_empty()
            && let Some(readme) = self.readme.clone()
        {
            ui.add_space(12.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::CODE_README))
                        .size(14.0)
                        .strong()
                        .color(palette.fg_default),
                );
                source_badge(ui, self.readme_source, &self.translator, palette);
            });
            ui.add_space(4.0);
            if let Some(content) = &readme.content {
                markdown(ui, &mut self.markdown_cache, content);
            } else if readme.binary {
                info_banner(ui, &self.translator.text(keys::CODE_BINARY), palette);
            } else if readme.truncated {
                info_banner(ui, &self.translator.text(keys::CODE_TOO_LARGE), palette);
            }
        }
    }

    fn code_file_view(&mut self, ui: &mut Ui, repository: &Repository) {
        let Some(path) = self.code_file_path.clone() else {
            return;
        };
        let palette = self.theme.palette;
        ui.add_space(10.0);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            icon(ui, Icon::File, 15.0, palette.fg_muted);
            ui.label(
                RichText::new(&path)
                    .monospace()
                    .size(12.0)
                    .strong()
                    .color(palette.fg_default),
            );
            source_badge(ui, self.file_source, &self.translator, palette);
            if let Some(file) = &self.code_file
                && let Some(url) = &file.html_url
                && ui
                    .add(
                        egui::Button::new(
                            RichText::new(self.translator.text(keys::COMMON_OPEN_GITHUB))
                                .size(11.0)
                                .color(palette.accent_fg),
                        )
                        .frame(false),
                    )
                    .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url.clone()));
            }
        });
        if self.file_pending.is_some() {
            loading(ui, &self.translator.text(keys::CODE_FILE_LOADING), palette);
            return;
        }
        if let Some(error) = &self.file_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
            return;
        }
        let Some(file) = &self.code_file else {
            return;
        };
        if file.binary {
            info_banner(ui, &self.translator.text(keys::CODE_BINARY), palette);
            return;
        }
        if file.truncated {
            info_banner(ui, &self.translator.text(keys::CODE_TOO_LARGE), palette);
            return;
        }
        if let Some(content) = &file.content {
            let content = if content.chars().count() > 50_000 {
                content.chars().take(50_000).collect::<String>()
            } else {
                content.clone()
            };
            ScrollArea::both()
                .id_salt(("code-file", repository.github_id, path))
                .max_height(340.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(content)
                                .monospace()
                                .size(11.0)
                                .color(palette.fg_default),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        } else {
            muted(ui, self.translator.text(keys::CODE_EMPTY), palette);
        }
    }

    fn branches_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        self.ensure_branches(repository);
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::BRANCHES_TITLE))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            source_badge(ui, self.branches_source, &self.translator, palette);
            if ui
                .add(secondary_button(
                    self.translator.text(keys::BRANCHES_REFRESH),
                    palette,
                ))
                .clicked()
            {
                refresh = true;
            }
        });
        if refresh {
            self.branches_repo = None;
            self.branches_pending = None;
            self.branches.clear();
            self.branches_source = DataSource::Remote;
            self.ensure_branches(repository);
        }
        if let Some(error) = &self.branches_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
        }
        if self.branches_pending.is_some() {
            loading(ui, &self.translator.text(keys::CODE_LOADING), palette);
            return;
        }
        if self.branches.is_empty() {
            empty_state(
                ui,
                &self.translator.text(keys::BRANCHES_EMPTY),
                "",
                palette,
                Some(Icon::Branch),
            );
            return;
        }
        let branches = self.branches.clone();
        let mut open_branch = None;
        for branch in branches {
            let is_default = branch.name == repository.default_branch;
            let is_selected = branch.name == self.selected_branch;
            Frame::NONE
                .fill(if is_selected {
                    palette.selection_bg
                } else {
                    palette.canvas_default
                })
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        icon(ui, Icon::Branch, 15.0, palette.fg_muted);
                        if ui
                            .selectable_label(
                                is_selected,
                                RichText::new(&branch.name)
                                    .size(12.5)
                                    .strong()
                                    .color(palette.fg_default),
                            )
                            .clicked()
                        {
                            open_branch = Some(branch.name.clone());
                        }
                        if is_default {
                            badge(
                                ui,
                                self.translator.text(keys::BRANCHES_DEFAULT),
                                palette.canvas_inset,
                                palette.fg_muted,
                            );
                        }
                        if branch.protected {
                            badge(
                                ui,
                                self.translator.text(keys::BRANCHES_PROTECTED),
                                palette.diff_add_bg,
                                palette.success_fg,
                            );
                        }
                        ui.label(
                            RichText::new(&branch.sha)
                                .monospace()
                                .size(10.5)
                                .color(palette.fg_subtle),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(
                                            self.translator.text(keys::BRANCHES_VIEW_CODE),
                                        )
                                        .size(11.0)
                                        .color(palette.accent_fg),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                open_branch = Some(branch.name.clone());
                            }
                        });
                    });
                });
            ui.separator();
        }
        if let Some(branch) = open_branch {
            self.switch_branch(repository, &branch);
            self.repo_tab = RepoTab::Code;
            self.ensure_code(repository);
        }
    }

    fn releases_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        self.load_releases(repository, false);
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::RELEASES_TITLE))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            source_badge(ui, self.releases_source, &self.translator, palette);
            if ui
                .add(secondary_button(
                    self.translator.text(keys::RELEASES_REFRESH),
                    palette,
                ))
                .clicked()
            {
                refresh = true;
            }
        });
        if refresh {
            self.load_releases(repository, true);
        }
        if let Some(error) = &self.releases_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
        }
        if self.releases_pending.is_some() {
            loading(ui, &self.translator.text(keys::CODE_LOADING), palette);
            return;
        }
        if self.releases.is_empty() {
            empty_state(
                ui,
                &self.translator.text(keys::RELEASES_EMPTY),
                "",
                palette,
                Some(Icon::Repo),
            );
            return;
        }
        let releases = self.releases.clone();
        for (index, release) in releases.into_iter().enumerate() {
            card(ui, palette, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(&release.tag_name)
                            .monospace()
                            .size(13.0)
                            .strong()
                            .color(palette.fg_default),
                    );
                    if let Some(name) = &release.name {
                        ui.label(RichText::new(name).size(12.0).color(palette.fg_muted));
                    }
                    if release.prerelease {
                        badge(
                            ui,
                            self.translator.text(keys::RELEASES_PRERELEASE),
                            palette.diff_del_bg,
                            palette.attention_fg,
                        );
                    }
                    if release.draft {
                        badge(
                            ui,
                            self.translator.text(keys::RELEASES_DRAFT),
                            palette.canvas_inset,
                            palette.fg_muted,
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(self.translator.text(keys::COMMON_OPEN_GITHUB))
                                        .size(11.0)
                                        .color(palette.accent_fg),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab(release.html_url.clone()));
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    let published = release.published_at.unwrap_or(release.created_at);
                    ui.label(
                        RichText::new(self.translator.text_args(
                            keys::RELEASES_PUBLISHED,
                            &[Arg::str(
                                "when",
                                &format_date(&self.translator, published, false),
                            )],
                        ))
                        .size(10.5)
                        .color(palette.fg_subtle),
                    );
                    if let Some(author) = &release.author {
                        ui.label(RichText::new(author).size(10.5).color(palette.fg_subtle));
                    }
                    ui.label(
                        RichText::new(self.translator.text_args(
                            keys::RELEASES_ASSETS,
                            &[Arg::int("count", release.assets.len() as i64)],
                        ))
                        .size(10.5)
                        .color(palette.fg_subtle),
                    );
                });
                ui.add_space(4.0);
                egui::CollapsingHeader::new(
                    RichText::new(self.translator.text(keys::RELEASES_NOTES))
                        .size(11.5)
                        .color(palette.fg_muted),
                )
                .id_salt(("release-body", release.id))
                .default_open(index == 0)
                .show(ui, |ui| {
                    if let Some(body) = &release.body {
                        if body.trim().is_empty() {
                            muted(ui, self.translator.text(keys::RELEASES_NO_BODY), palette);
                        } else {
                            markdown(ui, &mut self.markdown_cache, body);
                        }
                    } else {
                        muted(ui, self.translator.text(keys::RELEASES_NO_BODY), palette);
                    }
                });
            });
            ui.add_space(8.0);
        }
    }

    fn overview_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::TAB_OVERVIEW))
                    .size(14.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(6.0);
            kv_row(
                ui,
                &self.translator.text(keys::REPOS_DETAIL_DEFAULT_BRANCH),
                &repository.default_branch,
                palette,
            );
            kv_row(
                ui,
                &self.translator.text(keys::REPOS_DETAIL_LAST_PUSH),
                &format_date(
                    &self.translator,
                    repository.pushed_at.unwrap_or(repository.updated_at),
                    true,
                ),
                palette,
            );
            if let Some(synced_at) = repository.synced_at {
                kv_row(
                    ui,
                    &self.translator.text(keys::REPOS_DETAIL_SYNCED_AT),
                    &format_date(&self.translator, synced_at, true),
                    palette,
                );
            }
            if let Some(language) = &repository.language {
                kv_row(
                    ui,
                    &self.translator.text(keys::COMMON_LANGUAGE),
                    language,
                    palette,
                );
            }
            if let Some(error) = &repository.last_sync_error {
                ui.add_space(6.0);
                error_banner(
                    ui,
                    &self.translator.text(keys::REPOS_SYNC_STATUS_ERROR),
                    error,
                    palette,
                );
            }
        });
        ui.add_space(12.0);
        let (commits, additions, deletions) = self
            .db
            .commit_summary_for_branch(repository.github_id, &repository.default_branch)
            .unwrap_or((0, 0, 0));
        repo_stats(ui, &self.translator, commits, additions, deletions, palette);
        ui.add_space(12.0);
        ui.label(
            RichText::new(self.translator.text(keys::COMMITS_TITLE))
                .size(15.0)
                .strong()
                .color(palette.fg_default),
        );
        ui.add_space(4.0);
        let latest = self
            .db
            .list_commits_for_branch(repository.github_id, &repository.default_branch, 5, 0)
            .unwrap_or_default();
        if latest.is_empty() {
            muted(ui, self.translator.text(keys::REPOS_NO_COMMITS), palette);
        } else {
            for commit in latest {
                commit_row(ui, &commit, &self.translator, palette);
                ui.separator();
            }
        }
    }

    fn commits_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        self.ensure_commits(repository);
        let mut branch_changed = false;
        let mut commits_refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::COMMON_BRANCH))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            branch_changed = self.branch_combo(ui, repository);
            source_badge(ui, self.commits_source, &self.translator, palette);
            if ui
                .add(secondary_button(
                    self.translator.text(keys::COMMON_REFRESH),
                    palette,
                ))
                .clicked()
            {
                commits_refresh = true;
            }
        });
        if branch_changed {
            self.ensure_commits(repository);
        }
        if commits_refresh {
            self.commits_error = None;
            self.commits_source = DataSource::Remote;
            self.ensure_commits(repository);
        }
        if let Some(error) = &self.commits_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
        }
        if self.commits_source == DataSource::Loading {
            loading(ui, &self.translator.text(keys::COMMITS_LOADING), palette);
            return;
        }
        if self.commits.is_empty() {
            empty_state(
                ui,
                &self.translator.text(keys::COMMITS_EMPTY_TITLE),
                &self.translator.text(keys::COMMITS_EMPTY_BODY),
                palette,
                Some(Icon::Commit),
            );
            return;
        }
        let mut open_diff = None;
        ScrollArea::vertical()
            .id_salt(("commits", self.commits_branch.clone()))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for commit in &self.commits {
                    if commit_row(ui, commit, &self.translator, palette) {
                        open_diff = Some(commit.sha.clone());
                    }
                    ui.separator();
                }
            });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(self.translator.text_args(
                    keys::COMMITS_SHOWING,
                    &[
                        Arg::int("shown", self.commits.len() as i64),
                        Arg::int("total", i64::from(self.commits_total)),
                    ],
                ))
                .size(11.5)
                .color(palette.fg_muted),
            );
            if self.commits.len() < self.commits_total as usize
                && ui
                    .add(secondary_button(
                        self.translator.text(keys::COMMITS_LOAD_MORE),
                        palette,
                    ))
                    .clicked()
            {
                self.load_more_commits(repository.github_id, &self.commits_branch.clone());
            }
        });
        if let Some(sha) = open_diff {
            self.open_commit_diff(sha);
        }
    }

    fn issues_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        self.load_issues(repository, &self.issues_state.clone(), false);
        let mut state = self.issues_state.clone();
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::ISSUES_TITLE))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            source_badge(ui, self.issues_source, &self.translator, palette);
            for (value, key) in [
                ("open", keys::ISSUES_OPEN),
                ("closed", keys::ISSUES_CLOSED),
                ("all", keys::ISSUES_ALL),
            ] {
                ui.selectable_value(&mut state, value.to_owned(), self.translator.text(key));
            }
            if ui
                .add(secondary_button(
                    self.translator.text(keys::ISSUES_REFRESH),
                    palette,
                ))
                .clicked()
            {
                refresh = true;
            }
        });
        if state != self.issues_state {
            self.issues_state = state;
            self.issues.clear();
            self.issues_repo = None;
            self.issues_source = DataSource::Remote;
            self.issues_pending = None;
            self.selected_issue = None;
            self.issue_detail_pending = None;
            self.issue_comments_pending = None;
            self.load_issues(repository, &self.issues_state.clone(), false);
        } else if refresh {
            self.load_issues(repository, &self.issues_state.clone(), true);
        }
        if let Some(error) = &self.issues_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
        }
        if self.issues_source == DataSource::Loading {
            loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
        }
        if self.issues.is_empty() {
            if self.issues_source != DataSource::Loading {
                empty_state(
                    ui,
                    &self.translator.text(keys::ISSUES_EMPTY),
                    "",
                    palette,
                    Some(Icon::Warning),
                );
            }
        } else {
            let issues = self.issues.clone();
            let mut selected = None;
            ScrollArea::vertical()
                .id_salt(("issues", self.issues_state.clone()))
                .max_height(300.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for issue in issues {
                        if issue_row(ui, &issue, &self.translator, palette) {
                            selected = Some(issue);
                        }
                        ui.separator();
                    }
                });
            if let Some(issue) = selected {
                self.select_issue(repository, issue);
            }
        }
        #[cfg(debug_assertions)]
        if self.selected_issue.is_none()
            && std::env::var("GITPANEL_AUTO_ISSUE").is_ok()
            && let Some(issue) = self.issues.first().cloned()
        {
            self.select_issue(repository, issue);
        }

        if let Some(issue) = self.selected_issue.clone() {
            ui.add_space(12.0);
            card(ui, palette, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("#{} {}", issue.number, issue.title))
                            .size(15.0)
                            .strong()
                            .color(palette.fg_default),
                    );
                    badge(
                        ui,
                        if issue.state == "open" {
                            self.translator.text(keys::ISSUES_OPEN)
                        } else {
                            self.translator.text(keys::ISSUES_CLOSED)
                        },
                        if issue.state == "open" {
                            palette.diff_add_bg
                        } else {
                            palette.canvas_inset
                        },
                        if issue.state == "open" {
                            palette.success_fg
                        } else {
                            palette.fg_muted
                        },
                    );
                    source_badge(ui, self.issue_detail_source, &self.translator, palette);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(self.translator.text(keys::COMMON_OPEN_GITHUB))
                                        .size(11.0)
                                        .color(palette.accent_fg),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab(issue.html_url.clone()));
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(author) = &issue.user {
                        ui.label(
                            RichText::new(
                                self.translator.text_args(
                                    keys::ISSUES_OPENED_BY,
                                    &[Arg::str("author", author)],
                                ),
                            )
                            .size(11.0)
                            .color(palette.fg_muted),
                        );
                    }
                    label_chips(ui, &issue.labels, palette);
                });
                ui.add_space(6.0);
                if self.issue_detail_source == DataSource::Loading {
                    loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
                } else if let Some(error) = &self.issue_detail_error {
                    error_banner(
                        ui,
                        &self.translator.text(keys::ERROR_GENERIC),
                        error,
                        palette,
                    );
                } else if let Some(body) = &issue.body {
                    if body.trim().is_empty() {
                        muted(ui, self.translator.text(keys::ISSUES_NO_BODY), palette);
                    } else {
                        markdown(ui, &mut self.markdown_cache, body);
                    }
                } else {
                    muted(ui, self.translator.text(keys::ISSUES_NO_BODY), palette);
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text_args(
                        keys::ISSUES_COMMENTS,
                        &[Arg::int("count", i64::from(issue.comments))],
                    ))
                    .size(13.0)
                    .strong()
                    .color(palette.fg_default),
                );
                source_badge(ui, self.issue_comments_source, &self.translator, palette);
            });
            if self.issue_comments_source == DataSource::Loading {
                loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
            } else if let Some(error) = &self.issue_comments_error {
                error_banner(
                    ui,
                    &self.translator.text(keys::ERROR_GENERIC),
                    error,
                    palette,
                );
            } else {
                let comments = self.issue_comments.clone();
                for comment in comments {
                    card(ui, palette, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(comment.user.as_deref().unwrap_or("—"))
                                    .size(11.5)
                                    .strong()
                                    .color(palette.fg_default),
                            );
                            ui.label(
                                RichText::new(format_date(
                                    &self.translator,
                                    comment.created_at,
                                    true,
                                ))
                                .size(10.5)
                                .color(palette.fg_subtle),
                            );
                        });
                        ui.add_space(4.0);
                        if let Some(body) = &comment.body {
                            markdown(ui, &mut self.markdown_cache, body);
                        }
                    });
                    ui.add_space(6.0);
                }
            }
        } else {
            muted(ui, self.translator.text(keys::ISSUES_SELECT), palette);
        }
    }

    fn pulls_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        self.load_pulls(repository, &self.pulls_state.clone(), false);
        let mut state = self.pulls_state.clone();
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::PULLS_TITLE))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            source_badge(ui, self.pulls_source, &self.translator, palette);
            for (value, key) in [
                ("open", keys::PULLS_OPEN),
                ("closed", keys::PULLS_CLOSED),
                ("all", keys::PULLS_ALL),
            ] {
                ui.selectable_value(&mut state, value.to_owned(), self.translator.text(key));
            }
            if ui
                .add(secondary_button(
                    self.translator.text(keys::PULLS_REFRESH),
                    palette,
                ))
                .clicked()
            {
                refresh = true;
            }
        });
        if state != self.pulls_state {
            self.pulls_state = state;
            self.pulls.clear();
            self.pulls_repo = None;
            self.pulls_source = DataSource::Remote;
            self.pulls_pending = None;
            self.selected_pull = None;
            self.pull_detail_pending = None;
            self.pull_comments_pending = None;
            self.pull_files_pending = None;
            self.load_pulls(repository, &self.pulls_state.clone(), false);
        } else if refresh {
            self.load_pulls(repository, &self.pulls_state.clone(), true);
        }
        if let Some(error) = &self.pulls_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
        }
        if self.pulls_source == DataSource::Loading {
            loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
        }
        if self.pulls.is_empty() {
            if self.pulls_source != DataSource::Loading {
                empty_state(
                    ui,
                    &self.translator.text(keys::PULLS_EMPTY),
                    "",
                    palette,
                    Some(Icon::Branch),
                );
            }
        } else {
            let pulls = self.pulls.clone();
            let mut selected = None;
            ScrollArea::vertical()
                .id_salt(("pulls", self.pulls_state.clone()))
                .max_height(300.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for pull in pulls {
                        if pull_row(ui, &pull, &self.translator, palette) {
                            selected = Some(pull);
                        }
                        ui.separator();
                    }
                });
            if let Some(pull) = selected {
                self.select_pull(repository, pull);
            }
        }
        #[cfg(debug_assertions)]
        if self.selected_pull.is_none()
            && std::env::var("GITPANEL_AUTO_PULL").is_ok()
            && let Some(pull) = self.pulls.first().cloned()
        {
            self.select_pull(repository, pull.clone());
            if std::env::var("GITPANEL_AUTO_PULL_DIFF").is_ok() {
                self.load_pull_files(repository, &pull, false);
            }
        }

        if let Some(pull) = self.selected_pull.clone() {
            ui.add_space(12.0);
            card(ui, palette, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("#{} {}", pull.number, pull.title))
                            .size(15.0)
                            .strong()
                            .color(palette.fg_default),
                    );
                    if pull.merged {
                        badge(
                            ui,
                            self.translator.text(keys::PULLS_MERGED),
                            palette.selection_bg,
                            palette.done_fg,
                        );
                    } else {
                        badge(
                            ui,
                            if pull.state == "open" {
                                self.translator.text(keys::PULLS_OPEN)
                            } else {
                                self.translator.text(keys::PULLS_CLOSED)
                            },
                            if pull.state == "open" {
                                palette.diff_add_bg
                            } else {
                                palette.canvas_inset
                            },
                            if pull.state == "open" {
                                palette.success_fg
                            } else {
                                palette.fg_muted
                            },
                        );
                    }
                    if pull.draft {
                        badge(
                            ui,
                            self.translator.text(keys::PULLS_DRAFT),
                            palette.canvas_inset,
                            palette.fg_muted,
                        );
                    }
                    source_badge(ui, self.pull_detail_source, &self.translator, palette);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(self.translator.text(keys::COMMON_OPEN_GITHUB))
                                        .size(11.0)
                                        .color(palette.accent_fg),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab(pull.html_url.clone()));
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(author) = &pull.user {
                        ui.label(
                            RichText::new(
                                self.translator.text_args(
                                    keys::PULLS_OPENED_BY,
                                    &[Arg::str("author", author)],
                                ),
                            )
                            .size(11.0)
                            .color(palette.fg_muted),
                        );
                    }
                    ui.label(
                        RichText::new(self.translator.text_args(
                            keys::PULLS_FROM_TO,
                            &[
                                Arg::str("head", &pull.head_ref),
                                Arg::str("base", &pull.base_ref),
                            ],
                        ))
                        .size(11.0)
                        .color(palette.fg_muted),
                    );
                });
                ui.add_space(6.0);
                if self.pull_detail_source == DataSource::Loading {
                    loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
                } else if let Some(error) = &self.pull_detail_error {
                    error_banner(
                        ui,
                        &self.translator.text(keys::ERROR_GENERIC),
                        error,
                        palette,
                    );
                } else if let Some(body) = &pull.body {
                    if body.trim().is_empty() {
                        muted(ui, self.translator.text(keys::PULLS_NO_BODY), palette);
                    } else {
                        markdown(ui, &mut self.markdown_cache, body);
                    }
                } else {
                    muted(ui, self.translator.text(keys::PULLS_NO_BODY), palette);
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(self.translator.text_args(
                            keys::PULLS_COMMITS,
                            &[Arg::int("count", i64::from(pull.commits))],
                        ))
                        .size(11.0)
                        .color(palette.fg_muted),
                    );
                    ui.label(
                        RichText::new(self.translator.text_args(
                            keys::PULLS_FILES,
                            &[Arg::int("count", i64::from(pull.changed_files))],
                        ))
                        .size(11.0)
                        .color(palette.fg_muted),
                    );
                    ui.label(
                        RichText::new(format!("+{} −{}", pull.additions, pull.deletions))
                            .size(11.0)
                            .color(palette.fg_muted),
                    );
                    if ui
                        .add(secondary_button(
                            self.translator.text(keys::PULLS_VIEW_DIFF),
                            palette,
                        ))
                        .clicked()
                    {
                        self.load_pull_files(repository, &pull, false);
                    }
                    source_badge(ui, self.pull_files_source, &self.translator, palette);
                });
                if self.pull_files_source == DataSource::Loading {
                    loading(ui, &self.translator.text(keys::PULLS_LOADING_DIFF), palette);
                } else if let Some(error) = &self.pull_files_error {
                    error_banner(
                        ui,
                        &self.translator.text(keys::ERROR_GENERIC),
                        error,
                        palette,
                    );
                } else if let Some(diff) = &self.pull_diff {
                    if self.highlighter.is_none() {
                        self.highlighter = Some(Highlighter::new(self.theme.dark));
                    }
                    if let Some(highlighter) = &self.highlighter {
                        diff_view(
                            ui,
                            diff,
                            &self.translator,
                            palette,
                            highlighter,
                            &mut self.pull_diff_state,
                        );
                    }
                } else {
                    muted(ui, self.translator.text(keys::PULLS_NO_DIFF), palette);
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text_args(
                        keys::PULLS_COMMENTS,
                        &[Arg::int("count", i64::from(pull.comments))],
                    ))
                    .size(13.0)
                    .strong()
                    .color(palette.fg_default),
                );
                source_badge(ui, self.pull_comments_source, &self.translator, palette);
            });
            if self.pull_comments_source == DataSource::Loading {
                loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
            } else if let Some(error) = &self.pull_comments_error {
                error_banner(
                    ui,
                    &self.translator.text(keys::ERROR_GENERIC),
                    error,
                    palette,
                );
            } else {
                let comments = self.pull_comments.clone();
                for comment in comments {
                    card(ui, palette, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(comment.user.as_deref().unwrap_or("—"))
                                    .size(11.5)
                                    .strong()
                                    .color(palette.fg_default),
                            );
                            ui.label(
                                RichText::new(format_date(
                                    &self.translator,
                                    comment.created_at,
                                    true,
                                ))
                                .size(10.5)
                                .color(palette.fg_subtle),
                            );
                        });
                        ui.add_space(4.0);
                        if let Some(body) = &comment.body {
                            markdown(ui, &mut self.markdown_cache, body);
                        }
                    });
                    ui.add_space(6.0);
                }
            }
        } else {
            muted(ui, self.translator.text(keys::PULLS_SELECT), palette);
        }
    }

    fn actions_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        self.load_actions(repository, false);
        let mut filter = self.actions_filter.clone();
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::ACTIONS_TITLE))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            source_badge(ui, self.actions_source, &self.translator, palette);
            for (value, key) in [
                ("all", keys::ACTIONS_ALL),
                ("active", keys::ACTIONS_ACTIVE),
                ("completed", keys::ACTIONS_COMPLETED),
            ] {
                ui.selectable_value(&mut filter, value.to_owned(), self.translator.text(key));
            }
            if ui
                .add(secondary_button(
                    self.translator.text(keys::ACTIONS_REFRESH),
                    palette,
                ))
                .clicked()
            {
                refresh = true;
            }
        });
        self.actions_filter = filter;
        if refresh {
            self.load_actions(repository, true);
        }
        if let Some(error) = &self.actions_error {
            error_banner(
                ui,
                &self.translator.text(keys::ERROR_GENERIC),
                error,
                palette,
            );
        }
        if self.actions_source == DataSource::Loading {
            loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
        }
        let runs: Vec<WorkflowRun> = self
            .actions_runs
            .iter()
            .filter(|run| match self.actions_filter.as_str() {
                "active" => run.status != "completed",
                "completed" => run.status == "completed",
                _ => true,
            })
            .cloned()
            .collect();
        if runs.is_empty() {
            if self.actions_source != DataSource::Loading {
                empty_state(
                    ui,
                    &self.translator.text(keys::ACTIONS_EMPTY),
                    "",
                    palette,
                    Some(Icon::Refresh),
                );
            }
        } else {
            let mut selected = None;
            ScrollArea::vertical()
                .id_salt(("actions-runs", self.actions_filter.clone()))
                .max_height(280.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for run in &runs {
                        if action_run_row(
                            ui,
                            run,
                            &self.translator,
                            palette,
                            self.selected_run.as_ref().map(|selected| selected.id),
                        ) {
                            selected = Some(run.clone());
                        }
                        ui.separator();
                    }
                });
            if let Some(run) = selected {
                self.select_run(repository, run);
            }
        }

        if let Some(run) = self.selected_run.clone() {
            ui.add_space(12.0);
            card(ui, palette, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{}  ·  {}",
                            run.name,
                            self.translator.text_args(
                                keys::ACTIONS_RUN,
                                &[Arg::int("number", i64::from(run.run_number))],
                            )
                        ))
                        .size(15.0)
                        .strong()
                        .color(palette.fg_default),
                    );
                    action_status_badge(
                        ui,
                        &run.status,
                        run.conclusion.as_deref(),
                        &self.translator,
                        palette,
                    );
                    source_badge(ui, self.action_jobs_source, &self.translator, palette);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(self.translator.text(keys::COMMON_OPEN_GITHUB))
                                        .size(11.0)
                                        .color(palette.accent_fg),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            ui.ctx()
                                .open_url(egui::OpenUrl::new_tab(run.html_url.clone()));
                        }
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(title) = &run.display_title {
                        ui.label(RichText::new(title).size(11.5).color(palette.fg_muted));
                    }
                    ui.label(
                        RichText::new(
                            self.translator
                                .text_args(keys::ACTIONS_EVENT, &[Arg::str("event", &run.event)]),
                        )
                        .size(10.5)
                        .color(palette.fg_subtle),
                    );
                    if let Some(branch) = &run.head_branch {
                        ui.label(
                            RichText::new(
                                self.translator
                                    .text_args(keys::ACTIONS_BRANCH, &[Arg::str("branch", branch)]),
                            )
                            .size(10.5)
                            .color(palette.fg_subtle),
                        );
                    }
                    ui.label(
                        RichText::new(run.head_sha.get(..7).unwrap_or(&run.head_sha))
                            .monospace()
                            .size(10.5)
                            .color(palette.fg_subtle),
                    );
                    ui.label(
                        RichText::new(relative_time(&self.translator, run.updated_at, Utc::now()))
                            .size(10.5)
                            .color(palette.fg_subtle),
                    );
                });
                ui.add_space(6.0);
                if self.action_jobs_source == DataSource::Loading {
                    loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
                } else if let Some(error) = &self.action_jobs_error {
                    error_banner(
                        ui,
                        &self.translator.text(keys::ERROR_GENERIC),
                        error,
                        palette,
                    );
                } else if self.action_jobs.is_empty() {
                    muted(ui, self.translator.text(keys::ACTIONS_EMPTY), palette);
                } else {
                    let jobs = self.action_jobs.clone();
                    for job in &jobs {
                        action_job_card(
                            ui,
                            job,
                            &self.translator,
                            palette,
                            self.action_logs.contains_key(&job.id),
                        );
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(self.translator.text(keys::ACTIONS_LOGS))
                                        .size(11.0)
                                        .color(palette.accent_fg),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.load_job_logs(repository, job);
                        }
                    }
                }

                ui.add_space(10.0);
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.translator.text(keys::ACTIONS_LOGS))
                            .size(14.0)
                            .strong()
                            .color(palette.fg_default),
                    );
                    source_badge(ui, self.action_log_source, &self.translator, palette);
                });
                if self.action_log_source == DataSource::Loading {
                    loading(
                        ui,
                        &self.translator.text(keys::ACTIONS_LOADING_LOGS),
                        palette,
                    );
                } else if let Some(error) = &self.action_log_error {
                    error_banner(
                        ui,
                        &self.translator.text(keys::ERROR_GENERIC),
                        error,
                        palette,
                    );
                } else if let Some(job_id) = self.action_log_job
                    && let Some(log) = self.action_logs.get(&job_id)
                {
                    ScrollArea::both()
                        .id_salt(("action-log", job_id))
                        .max_height(300.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(log)
                                        .monospace()
                                        .size(10.5)
                                        .color(palette.fg_default),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        });
                } else {
                    muted(ui, self.translator.text(keys::ACTIONS_NO_LOGS), palette);
                }
            });
        }
    }

    fn charts_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        if self.charts.repo_id != Some(repository.github_id) {
            self.load_charts(repository.github_id);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::CHART_PERIOD))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            let periods = [
                (Some(7u32), keys::CHART_PERIOD_7D),
                (Some(30), keys::CHART_PERIOD_30D),
                (Some(90), keys::CHART_PERIOD_90D),
                (Some(365), keys::CHART_PERIOD_1Y),
                (None, keys::CHART_PERIOD_ALL),
            ];
            for (days, key) in periods {
                ui.selectable_value(
                    &mut self.charts.period_days,
                    days,
                    self.translator.text(key),
                );
            }
            ui.separator();
            ui.label(
                RichText::new(self.translator.text(keys::CHART_GRANULARITY))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            for (granularity, key) in [
                (Granularity::Minute, keys::CHART_GRANULARITY_MINUTE),
                (Granularity::Hour, keys::CHART_GRANULARITY_HOUR),
                (Granularity::Day, keys::CHART_GRANULARITY_DAY),
                (Granularity::Week, keys::CHART_GRANULARITY_WEEK),
                (Granularity::Month, keys::CHART_GRANULARITY_MONTH),
            ] {
                ui.selectable_value(
                    &mut self.charts.granularity,
                    granularity,
                    self.translator.text(key),
                );
            }
            ui.checkbox(
                &mut self.charts.include_merges,
                self.translator.text(keys::CHART_MERGE_TOGGLE),
            );
        });
        ui.add_space(8.0);
        if self.charts.commits.is_empty() {
            empty_state(
                ui,
                &self.translator.text(keys::CHART_EMPTY_TITLE),
                &self.translator.text(keys::CHART_EMPTY_BODY),
                palette,
                Some(Icon::Chart),
            );
            return;
        }
        let since = self
            .charts
            .period_days
            .and_then(|days| Utc::now().checked_sub_signed(TimeDelta::days(i64::from(days))));
        let points = aggregate_local(
            &self.charts.commits,
            self.charts.granularity,
            self.charts.include_merges,
            since,
            None,
        );
        let filtered: Vec<Commit> = self
            .charts
            .commits
            .iter()
            .filter(|commit| since.is_none_or(|start| commit.committed_at >= start))
            .cloned()
            .collect();
        let summary = summary(&filtered, self.charts.include_merges);
        let contributors = top_contributors(&filtered, self.charts.include_merges, 5);
        ui.label(
            RichText::new(self.translator.text(keys::CHART_BRANCH_NOTE))
                .size(11.5)
                .color(palette.fg_subtle),
        );
        commit_chart(
            ui,
            &points,
            self.charts.granularity,
            &self.translator,
            palette,
        );
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            stat_card(
                ui,
                &self.translator.text(keys::CHART_TOTAL_COMMITS),
                summary.commits.to_string(),
                None,
                palette,
            );
            stat_card(
                ui,
                &self.translator.text(keys::CHART_ADDITIONS),
                summary.additions.to_string(),
                None,
                palette,
            );
            stat_card(
                ui,
                &self.translator.text(keys::CHART_DELETIONS),
                summary.deletions.to_string(),
                None,
                palette,
            );
        });
        ui.add_space(12.0);
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::CHART_TOP_CONTRIBUTORS))
                    .size(14.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(6.0);
            for contributor in contributors {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&contributor.login)
                            .size(12.0)
                            .strong()
                            .color(palette.fg_default),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let commits = self.translator.text_args(
                            keys::COMMITS_COUNT,
                            &[Arg::int("count", i64::from(contributor.commits))],
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}  +{} −{}",
                                commits, contributor.additions, contributor.deletions
                            ))
                            .size(11.5)
                            .color(palette.fg_muted),
                        );
                    });
                });
            }
        });
    }

    fn diff_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(self.translator.text(keys::DIFF_SOURCE))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            ui.selectable_value(
                &mut self.diff_source,
                DiffSource::Commit,
                self.translator.text(keys::DIFF_SOURCE_COMMIT),
            );
            ui.selectable_value(
                &mut self.diff_source,
                DiffSource::Compare,
                self.translator.text(keys::DIFF_SOURCE_COMPARE),
            );
        });
        ui.add_space(6.0);
        match self.diff_source {
            DiffSource::Commit => {
                if self.commits.is_empty() {
                    muted(ui, self.translator.text(keys::REPOS_NO_COMMITS), palette);
                } else {
                    let selected = self
                        .selected_commit
                        .as_deref()
                        .and_then(|sha| {
                            self.commits
                                .iter()
                                .find(|commit| commit.sha == sha)
                                .map(|commit| format!("{}  {}", commit.short_sha(), commit.message))
                        })
                        .unwrap_or_else(|| self.translator.text(keys::DIFF_SELECT_COMMIT));
                    egui::ComboBox::from_id_salt("diff-commit")
                        .selected_text(selected)
                        .width(420.0)
                        .show_ui(ui, |ui| {
                            for commit in self.commits.iter().take(100) {
                                ui.selectable_value(
                                    &mut self.selected_commit,
                                    Some(commit.sha.clone()),
                                    format!("{}  {}", commit.short_sha(), commit.message),
                                );
                            }
                        });
                }
            }
            DiffSource::Compare => {
                self.ensure_branches(repository);
                if let Some(error) = &self.diff_error {
                    error_banner(ui, &self.translator.text(keys::ERROR_DIFF), error, palette);
                }
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.translator.text(keys::DIFF_BASE))
                            .size(12.0)
                            .color(palette.fg_muted),
                    );
                    egui::ComboBox::from_id_salt("diff-base")
                        .selected_text(if self.base_branch.is_empty() {
                            self.translator.text(keys::DIFF_SELECT_BASE)
                        } else {
                            self.base_branch.clone()
                        })
                        .show_ui(ui, |ui| {
                            for branch in &self.branches {
                                ui.selectable_value(
                                    &mut self.base_branch,
                                    branch.name.clone(),
                                    &branch.name,
                                );
                            }
                        });
                    ui.label(
                        RichText::new(self.translator.text(keys::DIFF_HEAD))
                            .size(12.0)
                            .color(palette.fg_muted),
                    );
                    egui::ComboBox::from_id_salt("diff-head")
                        .selected_text(if self.head_branch.is_empty() {
                            self.translator.text(keys::DIFF_SELECT_HEAD)
                        } else {
                            self.head_branch.clone()
                        })
                        .show_ui(ui, |ui| {
                            for branch in &self.branches {
                                ui.selectable_value(
                                    &mut self.head_branch,
                                    branch.name.clone(),
                                    &branch.name,
                                );
                            }
                        });
                });
            }
        }
        ui.add_space(6.0);
        if ui
            .add(primary_button(
                self.translator.text(keys::DIFF_LOAD),
                palette,
            ))
            .clicked()
        {
            self.start_diff();
        }
        ui.add_space(8.0);
        if self.diff_loading {
            loading(ui, &self.translator.text(keys::DIFF_LOADING), palette);
        } else if let Some(error) = &self.diff_error {
            error_banner(ui, &self.translator.text(keys::ERROR_DIFF), error, palette);
        } else if let Some(diff) = &self.diff {
            ui.horizontal_wrapped(|ui| {
                if !self.diff_label.is_empty() {
                    ui.label(
                        RichText::new(&self.diff_label)
                            .monospace()
                            .size(11.5)
                            .color(palette.fg_muted),
                    );
                }
                ui.label(
                    RichText::new(self.translator.text_args(
                        keys::DIFF_STATS,
                        &[
                            Arg::int("additions", diff.additions as i64),
                            Arg::int("deletions", diff.deletions as i64),
                        ],
                    ))
                    .size(11.5)
                    .color(palette.fg_muted),
                );
                ui.label(
                    RichText::new(self.translator.text_args(
                        keys::DIFF_FILE_COUNT,
                        &[Arg::int("count", diff.files.len() as i64)],
                    ))
                    .size(11.5)
                    .color(palette.fg_muted),
                );
            });
            if self.highlighter.is_none() {
                self.highlighter = Some(Highlighter::new(self.theme.dark));
            }
            if let Some(highlighter) = &self.highlighter {
                diff_view(
                    ui,
                    diff,
                    &self.translator,
                    palette,
                    highlighter,
                    &mut self.diff_state,
                );
            }
        } else {
            empty_state(
                ui,
                &self.translator.text(keys::DIFF_EMPTY_TITLE),
                &self.translator.text(keys::DIFF_EMPTY_BODY),
                palette,
                Some(Icon::Diff),
            );
        }
    }

    fn screen_settings(&mut self, ui: &mut Ui) {
        let palette = self.theme.palette;
        let mut form = self.settings.clone();
        let mut save = false;
        let mut reload = false;
        let mut clear_cache = false;
        let mut vacuum = false;
        let cache_stats = self.db.cache_stats().ok();
        page_header(
            ui,
            &self.translator.text(keys::SETTINGS_TITLE),
            &self.translator.text(keys::SETTINGS_SUBTITLE),
            palette,
        );
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_ACCOUNT))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(4.0);
            if let Some(account) = &self.account {
                kv_row(
                    ui,
                    &self.translator.text(keys::SETTINGS_ACCOUNT_LOGIN),
                    &account.login,
                    palette,
                );
                kv_row(
                    ui,
                    &self.translator.text(keys::SETTINGS_ACCOUNT_NAME),
                    account.display_name(),
                    palette,
                );
                kv_row(
                    ui,
                    &self.translator.text(keys::SETTINGS_ACCOUNT_SCOPES),
                    &account.scopes.join(", "),
                    palette,
                );
            } else {
                muted(
                    ui,
                    self.translator.text(keys::COMMON_NOT_AVAILABLE),
                    palette,
                );
            }
            muted(
                ui,
                self.translator.text(keys::SETTINGS_TOKEN_MEMORY),
                palette,
            );
            ui.add_space(6.0);
            if ui
                .add(secondary_button(
                    self.translator.text(keys::COMMON_RELOAD_TOKEN),
                    palette,
                ))
                .clicked()
            {
                reload = true;
            }
            if let Some(error) = &self.auth_error {
                ui.add_space(6.0);
                error_banner(ui, &self.translator.text(keys::ERROR_AUTH), error, palette);
            }
        });
        ui.add_space(12.0);
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_SYNC))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::SETTINGS_SYNC_INTERVAL))
                        .size(12.0)
                        .color(palette.fg_muted),
                );
                ui.add(egui::DragValue::new(&mut form.interval_minutes).range(5..=1440));
            });
            muted(
                ui,
                self.translator.text(keys::SETTINGS_SYNC_INTERVAL_HINT),
                palette,
            );
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::SETTINGS_SYNC_BACKFILL))
                        .size(12.0)
                        .color(palette.fg_muted),
                );
                ui.add(egui::DragValue::new(&mut form.backfill_days).range(1..=3650));
            });
            muted(
                ui,
                self.translator.text(keys::SETTINGS_SYNC_BACKFILL_HINT),
                palette,
            );
            ui.add_enabled(
                false,
                egui::Checkbox::new(
                    &mut form.sync_all,
                    self.translator.text(keys::SETTINGS_SYNC_ALL),
                ),
            );
            muted(
                ui,
                self.translator.text(keys::SETTINGS_SYNC_ALL_HINT),
                palette,
            );
        });
        ui.add_space(12.0);
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_APPEARANCE))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_THEME))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut form.theme,
                    ThemePreference::System,
                    self.translator.text(keys::SETTINGS_THEME_SYSTEM),
                );
                ui.radio_value(
                    &mut form.theme,
                    ThemePreference::Light,
                    self.translator.text(keys::SETTINGS_THEME_LIGHT),
                );
                ui.radio_value(
                    &mut form.theme,
                    ThemePreference::Dark,
                    self.translator.text(keys::SETTINGS_THEME_DARK),
                );
            });
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_LANGUAGE))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            let selected_language = match &form.language {
                LanguagePreference::System => self.translator.text(keys::SETTINGS_LANGUAGE_SYSTEM),
                LanguagePreference::Locale(id) => Translator::available()
                    .find(|locale| locale.id() == id)
                    .map(|locale| locale.name().to_owned())
                    .unwrap_or_else(|| id.clone()),
            };
            egui::ComboBox::from_id_salt("settings-language")
                .selected_text(selected_language)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut form.language,
                        LanguagePreference::System,
                        self.translator.text(keys::SETTINGS_LANGUAGE_SYSTEM),
                    );
                    for locale in Translator::available() {
                        ui.selectable_value(
                            &mut form.language,
                            LanguagePreference::Locale(locale.id().to_owned()),
                            locale.name(),
                        );
                    }
                });
        });
        ui.add_space(12.0);
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_CACHE))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(4.0);
            ui.checkbox(
                &mut form.cache_enabled,
                self.translator.text(keys::SETTINGS_CACHE_ENABLED),
            );
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::SETTINGS_CACHE_KEEP))
                        .size(12.0)
                        .color(palette.fg_muted),
                );
                ui.add(egui::DragValue::new(&mut form.cache_keep_days).range(1..=365));
            });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::SETTINGS_CACHE_MAX_FILE))
                        .size(12.0)
                        .color(palette.fg_muted),
                );
                ui.add(egui::DragValue::new(&mut form.cache_max_file_kb).range(16..=4096));
            });
            muted(ui, self.translator.text(keys::SETTINGS_CACHE_HINT), palette);
            if let Some(stats) = cache_stats {
                kv_row(
                    ui,
                    &self.translator.text(keys::SETTINGS_CACHE_DB_SIZE),
                    &format_bytes(stats.database_bytes),
                    palette,
                );
                kv_row(
                    ui,
                    &self.translator.text(keys::SETTINGS_CACHE),
                    &self.translator.text_args(
                        keys::SETTINGS_CACHE_ROWS,
                        &[Arg::int("count", stats.total_rows() as i64)],
                    ),
                    palette,
                );
            }
            ui.horizontal(|ui| {
                if ui
                    .add(secondary_button(
                        self.translator.text(keys::SETTINGS_CACHE_CLEAR),
                        palette,
                    ))
                    .clicked()
                {
                    clear_cache = true;
                }
                if ui
                    .add(secondary_button(
                        self.translator.text(keys::SETTINGS_CACHE_VACUUM),
                        palette,
                    ))
                    .clicked()
                {
                    vacuum = true;
                }
            });
        });
        ui.add_space(12.0);
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_PATHS))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(4.0);
            kv_row(
                ui,
                &self.translator.text(keys::SETTINGS_DATA_DIR),
                &self.config.paths.data_dir.display().to_string(),
                palette,
            );
            kv_row(
                ui,
                &self.translator.text(keys::SETTINGS_CACHE_DIR),
                &self.config.paths.cache_dir.display().to_string(),
                palette,
            );
            kv_row(
                ui,
                &self.translator.text(keys::SETTINGS_CONFIG_FILE),
                &self.config.paths.config_file.display().to_string(),
                palette,
            );
        });
        ui.add_space(12.0);
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_GITHUB))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.add_space(4.0);
            kv_row(
                ui,
                &self.translator.text(keys::SETTINGS_GITHUB_HOST),
                "github.com",
                palette,
            );
            if let Some(rate) = &self.sync_state.rate_limit {
                kv_row(
                    ui,
                    &self.translator.text(keys::SETTINGS_RATE_LIMIT),
                    &self.translator.text_args(
                        keys::SETTINGS_RATE_LIMIT_REST,
                        &[
                            Arg::int("remaining", i64::from(rate.remaining)),
                            Arg::int("limit", i64::from(rate.limit)),
                        ],
                    ),
                    palette,
                );
                kv_row(
                    ui,
                    &self.translator.text(keys::COMMON_RESET_AT),
                    &format_date(&self.translator, rate.reset_at, true),
                    palette,
                );
            } else {
                muted(
                    ui,
                    self.translator.text(keys::COMMON_NOT_AVAILABLE),
                    palette,
                );
            }
        });
        ui.add_space(12.0);
        card(ui, palette, |ui| {
            ui.label(
                RichText::new(self.translator.text(keys::SETTINGS_ABOUT))
                    .size(15.0)
                    .strong()
                    .color(palette.fg_default),
            );
            kv_row(
                ui,
                &self.translator.text(keys::SETTINGS_VERSION),
                env!("CARGO_PKG_VERSION"),
                palette,
            );
            kv_row(
                ui,
                &self.translator.text(keys::SETTINGS_LICENSE),
                "MIT OR Apache-2.0",
                palette,
            );
        });
        ui.add_space(16.0);
        if ui
            .add(primary_button(
                self.translator.text(keys::SETTINGS_SAVE),
                palette,
            ))
            .clicked()
        {
            save = true;
        }
        if reload {
            self.reload_auth_action();
        }
        if clear_cache {
            match self.db.clear_cache() {
                Ok(_) => self.toast(
                    self.translator.text(keys::SETTINGS_CACHE_CLEARED),
                    ToastKind::Success,
                ),
                Err(error) => self.toast(
                    format!("{}: {error}", self.translator.text(keys::ERROR_DB)),
                    ToastKind::Error,
                ),
            }
        }
        if vacuum {
            match self.db.vacuum() {
                Ok(()) => self.toast(
                    self.translator.text(keys::SETTINGS_CACHE_VACUUMED),
                    ToastKind::Success,
                ),
                Err(error) => self.toast(
                    format!("{}: {error}", self.translator.text(keys::ERROR_DB)),
                    ToastKind::Error,
                ),
            }
        }
        if save {
            self.save_settings(form);
        }
    }

    fn create_modal(&mut self, ctx: &egui::Context) {
        if !self.create_open {
            return;
        }
        let palette = self.theme.palette;
        let mut form = self.create_form.clone();
        let pending = self.create_pending.is_some();
        let mut submit = false;
        let mut close = false;
        let modal = egui::Modal::new(Id::unique("create-repository")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(
                RichText::new(self.translator.text(keys::REPOS_CREATE_TITLE))
                    .size(19.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.label(
                RichText::new(self.translator.text(keys::REPOS_CREATE_SUBTITLE))
                    .size(12.0)
                    .color(palette.fg_muted),
            );
            ui.add_space(10.0);
            ui.label(
                RichText::new(self.translator.text(keys::REPOS_CREATE_NAME))
                    .size(12.0)
                    .color(palette.fg_default),
            );
            ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(f32::INFINITY));
            muted(
                ui,
                self.translator.text(keys::REPOS_CREATE_NAME_HINT),
                palette,
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(self.translator.text(keys::REPOS_CREATE_DESCRIPTION))
                    .size(12.0)
                    .color(palette.fg_default),
            );
            ui.add(
                egui::TextEdit::multiline(&mut form.description)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(self.translator.text(keys::REPOS_CREATE_VISIBILITY))
                    .size(12.0)
                    .color(palette.fg_default),
            );
            ui.horizontal(|ui| {
                ui.radio_value(
                    &mut form.private,
                    false,
                    self.translator.text(keys::REPOS_CREATE_PUBLIC),
                );
                ui.radio_value(
                    &mut form.private,
                    true,
                    self.translator.text(keys::REPOS_CREATE_PRIVATE),
                );
            });
            ui.checkbox(
                &mut form.auto_init,
                self.translator.text(keys::REPOS_CREATE_README),
            );
            if !form.auto_init {
                muted(
                    ui,
                    self.translator.text(keys::REPOS_CREATE_EMPTY_NOTE),
                    palette,
                );
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::REPOS_CREATE_GITIGNORE))
                        .size(12.0)
                        .color(palette.fg_default),
                );
                egui::ComboBox::from_id_salt("create-gitignore")
                    .selected_text(if form.gitignore.is_empty() {
                        self.translator.text(keys::REPOS_CREATE_NO_TEMPLATE)
                    } else {
                        form.gitignore.clone()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut form.gitignore,
                            String::new(),
                            self.translator.text(keys::REPOS_CREATE_NO_TEMPLATE),
                        );
                        for template in ["Rust", "Node", "Python", "Go", "Java", "C++", "macOS"] {
                            ui.selectable_value(&mut form.gitignore, template.to_owned(), template);
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(self.translator.text(keys::REPOS_CREATE_LICENSE))
                        .size(12.0)
                        .color(palette.fg_default),
                );
                egui::ComboBox::from_id_salt("create-license")
                    .selected_text(if form.license.is_empty() {
                        self.translator.text(keys::REPOS_CREATE_NO_TEMPLATE)
                    } else {
                        form.license.clone()
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut form.license,
                            String::new(),
                            self.translator.text(keys::REPOS_CREATE_NO_TEMPLATE),
                        );
                        for (value, label) in [
                            ("mit", "MIT"),
                            ("apache-2.0", "Apache-2.0"),
                            ("gpl-3.0", "GPL-3.0"),
                            ("bsd-3-clause", "BSD-3-Clause"),
                        ] {
                            ui.selectable_value(&mut form.license, value.to_owned(), label);
                        }
                    });
            });
            if let Some(error) = &form.error {
                ui.add_space(6.0);
                error_banner(
                    ui,
                    &self.translator.text(keys::REPOS_CREATE_FAILED),
                    error,
                    palette,
                );
            }
            if pending {
                ui.add_space(6.0);
                loading(ui, &self.translator.text(keys::COMMON_LOADING), palette);
            }
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !pending,
                        primary_button(self.translator.text(keys::REPOS_CREATE_SUBMIT), palette),
                    )
                    .clicked()
                {
                    submit = true;
                }
                if ui
                    .add_enabled(
                        !pending,
                        secondary_button(self.translator.text(keys::COMMON_CANCEL), palette),
                    )
                    .clicked()
                {
                    close = true;
                }
            });
        });
        if modal.backdrop_response.clicked() && !pending {
            close = true;
        }
        self.create_form = form;
        if close {
            self.create_open = false;
            self.create_pending = None;
        }
        if submit {
            self.submit_create();
        }
    }

    fn rebuild_layout(&mut self, ctx: &egui::Context) {
        if self.screen == Screen::Repository {
            let Some(repository) = self.selected_repository().cloned() else {
                self.screen = Screen::Repositories;
                return;
            };
            match self.repo_tab {
                RepoTab::Code => self.ensure_code(&repository),
                RepoTab::Commits => self.ensure_commits(&repository),
                RepoTab::Branches => self.ensure_branches(&repository),
                RepoTab::Releases => self.load_releases(&repository, false),
                RepoTab::Issues => self.load_issues(&repository, &self.issues_state.clone(), false),
                RepoTab::Pulls => self.load_pulls(&repository, &self.pulls_state.clone(), false),
                RepoTab::Actions => self.load_actions(&repository, false),
                RepoTab::Charts => {
                    if self.charts.repo_id != Some(repository.github_id) {
                        self.load_charts(repository.github_id);
                    }
                }
                RepoTab::Diff => {
                    self.ensure_branches(&repository);
                    self.ensure_commits(&repository);
                }
                RepoTab::Overview => {}
            }
        }
        self.refresh_theme(ctx);
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        let palette = self.theme.palette;
        let mut go_repositories = false;
        let mut go_settings = false;
        let mut toggle_theme = false;
        egui::Panel::top("gitpanel-top")
            .exact_size(54.0)
            .resizable(false)
            .frame(
                Frame::NONE
                    .fill(palette.canvas_subtle)
                    .inner_margin(Margin::symmetric(18, 8))
                    .stroke(Stroke::new(1.0, palette.border_default)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.translator.text(keys::APP_NAME))
                            .size(17.0)
                            .strong()
                            .color(palette.fg_default),
                    );
                    ui.add_space(12.0);
                    if ui
                        .selectable_label(
                            matches!(self.screen, Screen::Repositories | Screen::Repository),
                            RichText::new(self.translator.text(keys::COMMON_REPOSITORIES))
                                .size(13.0),
                        )
                        .clicked()
                    {
                        go_repositories = true;
                    }
                    if ui
                        .selectable_label(
                            self.screen == Screen::Settings,
                            RichText::new(self.translator.text(keys::COMMON_SETTINGS)).size(13.0),
                        )
                        .clicked()
                    {
                        go_settings = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(account) = &self.account {
                            let (rect, response) = ui
                                .allocate_exact_size(egui::Vec2::splat(28.0), egui::Sense::click());
                            ui.painter().circle_filled(
                                rect.center(),
                                14.0,
                                palette.accent_emphasis,
                            );
                            let initial = account
                                .login
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_ascii_uppercase();
                            ui.painter().text(
                                rect.center(),
                                Align2::CENTER_CENTER,
                                initial,
                                FontId::proportional(12.0),
                                Color32::WHITE,
                            );
                            if response.on_hover_text(&account.login).clicked() {
                                go_settings = true;
                            }
                        } else {
                            icon(ui, Icon::Warning, 18.0, palette.attention_fg);
                        }
                        if icon_button(
                            ui,
                            if self.theme.dark {
                                Icon::Sun
                            } else {
                                Icon::Moon
                            },
                            &self.translator.text(if self.theme.dark {
                                keys::COMMON_LIGHT
                            } else {
                                keys::COMMON_DARK
                            }),
                        )
                        .clicked()
                        {
                            toggle_theme = true;
                        }
                        if self.sync_state.running {
                            ui.spinner();
                            ui.label(
                                RichText::new(self.translator.text(keys::SYNC_RUNNING))
                                    .size(11.5)
                                    .color(palette.fg_muted),
                            );
                        } else if let Some(until) = self.sync_state.paused_until {
                            icon(ui, Icon::Warning, 15.0, palette.attention_fg);
                            ui.label(
                                RichText::new(relative_time(&self.translator, until, Utc::now()))
                                    .size(11.5)
                                    .color(palette.fg_muted),
                            );
                        } else {
                            icon(ui, Icon::Check, 15.0, palette.success_fg);
                            ui.label(
                                RichText::new(self.translator.text(keys::SYNC_IDLE))
                                    .size(11.5)
                                    .color(palette.fg_muted),
                            );
                        }
                    });
                });
            });
        if go_repositories {
            self.screen = Screen::Repositories;
            self.scroll_generation = self.scroll_generation.wrapping_add(1);
        }
        if go_settings {
            self.screen = Screen::Settings;
            self.scroll_generation = self.scroll_generation.wrapping_add(1);
        }
        if toggle_theme {
            self.toggle_theme(ui.ctx());
        }
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }
        let palette = self.theme.palette;
        egui::Area::new(Id::unique("gitpanel-toasts"))
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for toast in &self.toasts {
                    let (background, foreground) = match toast.kind {
                        ToastKind::Success => (palette.diff_add_bg, palette.success_fg),
                        ToastKind::Error => (palette.diff_del_bg, palette.danger_fg),
                        ToastKind::Info => (palette.canvas_subtle, palette.fg_default),
                    };
                    Frame::NONE
                        .fill(background)
                        .stroke(Stroke::new(1.0, foreground))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&toast.text).size(12.0).color(foreground));
                        });
                    ui.add_space(4.0);
                }
            });
    }
}

impl eframe::App for GitPanelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_background();
        self.rebuild_layout(&ctx);
        self.top_bar(ui);
        egui::CentralPanel::default()
            .frame(panel_frame(self.theme.palette))
            .show(ui, |ui| match self.screen {
                Screen::Repositories => {
                    if self.repo_view == RepoView::Grid {
                        ScrollArea::vertical()
                            .id_salt("repositories-page")
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.screen_repositories(ui));
                    } else {
                        self.screen_repositories(ui);
                    }
                }
                Screen::Repository => {
                    ScrollArea::vertical()
                        .id_salt(("repository-scroll", self.scroll_generation))
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.screen_repository(ui, &ctx));
                }
                Screen::Settings => {
                    ScrollArea::vertical()
                        .id_salt(("settings-scroll", self.scroll_generation))
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.screen_settings(ui));
                }
            });
        self.create_modal(&ctx);
        self.draw_toasts(&ctx);
        let busy = self.sync_state.running
            || self.diff_loading
            || self.create_pending.is_some()
            || self.account_pending.is_some()
            || self.branches_pending.is_some();
        ctx.request_repaint_after(Duration::from_millis(if busy { 200 } else { 1000 }));
    }
}

fn action_status_badge(
    ui: &mut Ui,
    status: &str,
    conclusion: Option<&str>,
    translator: &Translator,
    palette: Palette,
) {
    let (key, background, foreground) = match (status, conclusion) {
        ("completed", Some("success")) => (
            keys::ACTIONS_STATUS_SUCCESS,
            palette.diff_add_bg,
            palette.success_fg,
        ),
        ("completed", Some("failure")) => (
            keys::ACTIONS_STATUS_FAILURE,
            palette.diff_del_bg,
            palette.danger_fg,
        ),
        ("completed", Some("cancelled")) => (
            keys::ACTIONS_STATUS_CANCELLED,
            palette.canvas_inset,
            palette.fg_muted,
        ),
        ("completed", Some("skipped")) => (
            keys::ACTIONS_STATUS_SKIPPED,
            palette.canvas_inset,
            palette.fg_muted,
        ),
        ("completed", Some("timed_out")) => (
            keys::ACTIONS_STATUS_TIMED_OUT,
            palette.diff_del_bg,
            palette.danger_fg,
        ),
        ("completed", Some("action_required")) => (
            keys::ACTIONS_STATUS_ACTION_REQUIRED,
            palette.canvas_inset,
            palette.attention_fg,
        ),
        ("completed", Some("neutral")) => (
            keys::ACTIONS_STATUS_NEUTRAL,
            palette.canvas_inset,
            palette.fg_muted,
        ),
        ("completed", Some("stale")) => (
            keys::ACTIONS_STATUS_STALE,
            palette.canvas_inset,
            palette.fg_muted,
        ),
        ("completed", _) => (
            keys::ACTIONS_STATUS_COMPLETED,
            palette.canvas_inset,
            palette.fg_muted,
        ),
        ("in_progress", _) => (
            keys::ACTIONS_STATUS_IN_PROGRESS,
            palette.selection_bg,
            palette.accent_fg,
        ),
        ("queued", _) | ("requested", _) | ("waiting", _) | ("pending", _) => (
            keys::ACTIONS_STATUS_QUEUED,
            palette.canvas_inset,
            palette.attention_fg,
        ),
        _ => (
            keys::ACTIONS_STATUS_UNKNOWN,
            palette.canvas_inset,
            palette.fg_muted,
        ),
    };
    badge(ui, translator.text(key), background, foreground);
}

fn action_run_row(
    ui: &mut Ui,
    run: &WorkflowRun,
    translator: &Translator,
    palette: Palette,
    selected: Option<i64>,
) -> bool {
    let mut clicked = false;
    Frame::NONE
        .fill(if selected == Some(run.id) {
            palette.selection_bg
        } else {
            palette.canvas_default
        })
        .inner_margin(Margin::symmetric(6, 7))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                action_status_badge(
                    ui,
                    &run.status,
                    run.conclusion.as_deref(),
                    translator,
                    palette,
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(&run.name)
                                .size(12.5)
                                .strong()
                                .color(palette.fg_default),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    clicked = true;
                }
                ui.label(
                    RichText::new(format!("#{}", run.run_number))
                        .monospace()
                        .size(10.5)
                        .color(palette.fg_subtle),
                );
                if let Some(title) = &run.display_title {
                    ui.label(RichText::new(title).size(10.5).color(palette.fg_muted));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(relative_time(translator, run.updated_at, Utc::now()))
                            .size(10.5)
                            .color(palette.fg_subtle),
                    );
                    if let Some(actor) = &run.actor {
                        ui.label(RichText::new(actor).size(10.5).color(palette.fg_muted));
                    }
                    if let Some(branch) = &run.head_branch {
                        ui.label(RichText::new(branch).size(10.5).color(palette.fg_subtle));
                    }
                    ui.label(
                        RichText::new(&run.event)
                            .size(10.5)
                            .color(palette.fg_subtle),
                    );
                });
            });
        });
    clicked
}

fn action_job_card(
    ui: &mut Ui,
    job: &WorkflowJob,
    translator: &Translator,
    palette: Palette,
    has_logs: bool,
) {
    card(ui, palette, |ui| {
        ui.horizontal_wrapped(|ui| {
            action_status_badge(
                ui,
                &job.status,
                job.conclusion.as_deref(),
                translator,
                palette,
            );
            ui.label(
                RichText::new(&job.name)
                    .size(12.5)
                    .strong()
                    .color(palette.fg_default),
            );
            if let Some(runner) = &job.runner_name {
                ui.label(
                    RichText::new(
                        translator.text_args(keys::ACTIONS_RUNNER, &[Arg::str("name", runner)]),
                    )
                    .size(10.5)
                    .color(palette.fg_subtle),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if has_logs {
                    badge(
                        ui,
                        translator.text(keys::ACTIONS_LOGS),
                        palette.diff_add_bg,
                        palette.success_fg,
                    );
                }
            });
        });
        if !job.steps.is_empty() {
            ui.add_space(4.0);
            for step in &job.steps {
                ui.horizontal(|ui| {
                    let (icon_kind, color) = match step.conclusion.as_deref() {
                        Some("success") => (Icon::Check, palette.success_fg),
                        Some("failure") | Some("timed_out") => (Icon::Warning, palette.danger_fg),
                        Some("skipped") => (Icon::Branch, palette.fg_subtle),
                        _ => (Icon::Branch, palette.attention_fg),
                    };
                    icon(ui, icon_kind, 12.0, color);
                    ui.label(RichText::new(&step.name).size(10.5).color(palette.fg_muted));
                });
            }
        }
    });
}

fn issue_row(ui: &mut Ui, issue: &Issue, translator: &Translator, palette: Palette) -> bool {
    let mut clicked = false;
    Frame::NONE
        .inner_margin(Margin::symmetric(4, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                icon(
                    ui,
                    if issue.state == "open" {
                        Icon::Warning
                    } else {
                        Icon::Check
                    },
                    14.0,
                    if issue.state == "open" {
                        palette.success_fg
                    } else {
                        palette.fg_muted
                    },
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("#{} {}", issue.number, issue.title))
                                .size(12.5)
                                .strong()
                                .color(palette.fg_default),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    clicked = true;
                }
                label_chips(ui, &issue.labels, palette);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(translator.text_args(
                            keys::ISSUES_COMMENTS,
                            &[Arg::int("count", i64::from(issue.comments))],
                        ))
                        .size(10.5)
                        .color(palette.fg_subtle),
                    );
                    ui.label(
                        RichText::new(relative_time(translator, issue.updated_at, Utc::now()))
                            .size(10.5)
                            .color(palette.fg_subtle),
                    );
                    if let Some(author) = &issue.user {
                        ui.label(RichText::new(author).size(10.5).color(palette.fg_muted));
                    }
                });
            });
        });
    clicked
}

fn pull_row(ui: &mut Ui, pull: &PullRequest, translator: &Translator, palette: Palette) -> bool {
    let mut clicked = false;
    Frame::NONE
        .inner_margin(Margin::symmetric(4, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                icon(
                    ui,
                    if pull.merged {
                        Icon::Check
                    } else {
                        Icon::Branch
                    },
                    14.0,
                    if pull.merged {
                        palette.done_fg
                    } else if pull.state == "open" {
                        palette.success_fg
                    } else {
                        palette.fg_muted
                    },
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!("#{} {}", pull.number, pull.title))
                                .size(12.5)
                                .strong()
                                .color(palette.fg_default),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    clicked = true;
                }
                if pull.merged {
                    badge(
                        ui,
                        translator.text(keys::PULLS_MERGED),
                        palette.selection_bg,
                        palette.done_fg,
                    );
                } else if pull.draft {
                    badge(
                        ui,
                        translator.text(keys::PULLS_DRAFT),
                        palette.canvas_inset,
                        palette.fg_muted,
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(translator.text_args(
                            keys::PULLS_COMMENTS,
                            &[Arg::int("count", i64::from(pull.comments))],
                        ))
                        .size(10.5)
                        .color(palette.fg_subtle),
                    );
                    ui.label(
                        RichText::new(relative_time(translator, pull.updated_at, Utc::now()))
                            .size(10.5)
                            .color(palette.fg_subtle),
                    );
                    if let Some(author) = &pull.user {
                        ui.label(RichText::new(author).size(10.5).color(palette.fg_muted));
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(translator.text_args(
                        keys::PULLS_FROM_TO,
                        &[
                            Arg::str("head", &pull.head_ref),
                            Arg::str("base", &pull.base_ref),
                        ],
                    ))
                    .size(10.5)
                    .color(palette.fg_subtle),
                );
                if pull.additions > 0 || pull.deletions > 0 {
                    ui.label(
                        RichText::new(format!("+{} −{}", pull.additions, pull.deletions))
                            .size(10.5)
                            .color(palette.fg_subtle),
                    );
                }
            });
        });
    clicked
}

fn label_chips(ui: &mut Ui, labels: &[Label], palette: Palette) {
    for label in labels {
        let color = parse_label_color(&label.color).unwrap_or(palette.canvas_inset);
        let foreground = if color.r() as u16 + color.g() as u16 + color.b() as u16 > 380 {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        badge(ui, &label.name, color, foreground);
    }
}

fn parse_label_color(value: &str) -> Option<Color32> {
    let value = value.trim_start_matches('#');
    if value.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&value[0..2], 16).ok()?;
    let green = u8::from_str_radix(&value[2..4], 16).ok()?;
    let blue = u8::from_str_radix(&value[4..6], 16).ok()?;
    Some(Color32::from_rgb(red, green, blue))
}

fn commit_row(ui: &mut Ui, commit: &Commit, translator: &Translator, palette: Palette) -> bool {
    let mut open = false;
    Frame::NONE
        .inner_margin(Margin::symmetric(4, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icon(ui, Icon::Commit, 16.0, palette.fg_muted);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&commit.message)
                            .size(12.5)
                            .strong()
                            .color(palette.fg_default),
                    );
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(commit.short_sha())
                                .monospace()
                                .size(10.5)
                                .color(palette.fg_subtle),
                        );
                        ui.label(
                            RichText::new(commit.author_display())
                                .size(11.0)
                                .color(palette.fg_muted),
                        );
                        ui.label(
                            RichText::new(relative_time(
                                translator,
                                commit.committed_at,
                                Utc::now(),
                            ))
                            .size(11.0)
                            .color(palette.fg_subtle),
                        );
                    });
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(translator.text(keys::COMMITS_OPEN_DIFF))
                                    .size(11.5)
                                    .color(palette.accent_fg),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        open = true;
                    }
                    ui.label(
                        RichText::new(format!("+{} −{}", commit.additions, commit.deletions))
                            .size(11.0)
                            .color(palette.fg_muted),
                    );
                });
            });
        });
    open
}

fn translator_commit_label(translator: &Translator, sha: &str) -> String {
    let short = sha.get(..7).unwrap_or(sha);
    translator.text_args(keys::DIFF_COMMIT_LABEL, &[Arg::str("sha", short)])
}

fn translation_key_for_code(code: &str) -> &'static str {
    match code {
        "error-gh-missing" => keys::ERROR_GH_MISSING,
        "error-gh-not-logged-in" => keys::ERROR_GH_NOT_LOGGED_IN,
        "error-network" => keys::ERROR_NETWORK,
        "error-auth" => keys::ERROR_AUTH,
        "error-forbidden" => keys::ERROR_FORBIDDEN,
        "error-not-found" => keys::ERROR_NOT_FOUND,
        "error-rate-limit" => keys::ERROR_RATE_LIMIT,
        "error-validation" => keys::ERROR_VALIDATION,
        "error-db" => keys::ERROR_DB,
        "error-config" => keys::ERROR_CONFIG,
        "error-i18n" => keys::ERROR_I18N,
        "error-sync" => keys::ERROR_SYNC,
        "error-create" => keys::ERROR_CREATE,
        "error-diff" => keys::ERROR_DIFF,
        "error-graphql" => keys::ERROR_GRAPHQL,
        "error-decode" => keys::ERROR_DECODE,
        "error-scope-missing" => keys::ERROR_SCOPE_MISSING,
        _ => keys::ERROR_GENERIC,
    }
}

pub struct FatalApp {
    message: String,
}

impl FatalApp {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl eframe::App for FatalApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new("GitPanel").size(28.0).strong());
                ui.add_space(8.0);
                ui.label(RichText::new("Não foi possível iniciar o aplicativo.").size(15.0));
                ui.add_space(8.0);
                ui.label(RichText::new(&self.message).monospace().size(12.0));
            });
        });
    }
}
