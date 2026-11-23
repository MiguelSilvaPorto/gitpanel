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
    Account, Branch, Commit, CreateRepoRequest, FilePatch, RateLimit, Repository, ValidationError,
    validate_description, validate_repo_name,
};
use gitpanel_db::Db;
use gitpanel_diff::{Diff, Highlighter};
use gitpanel_github::{GithubClient, GithubError};
use gitpanel_i18n::{Arg, LanguagePreference, Translator, keys};
use gitpanel_metrics::{Granularity, aggregate_local, summary, top_contributors};
use gitpanel_sync::{SyncEvent, SyncHandle};
use gitpanel_ui::{
    DiffViewState, Icon, Palette, Theme, badge, card, commit_chart, diff_view, empty_state,
    error_banner, format_date, icon, icon_button, info_banner, install_fonts, kv_row, loading,
    muted, outline_badge, page_header, panel_frame, primary_button, relative_time, repo_stats,
    repo_table, search_input, secondary_button, stat_card, tabs,
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
    Commits,
    Charts,
    Diff,
}

impl RepoTab {
    fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Commits => 1,
            Self::Charts => 2,
            Self::Diff => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Commits,
            2 => Self::Charts,
            3 => Self::Diff,
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
}

impl SettingsForm {
    fn from_config(config: &Config) -> Self {
        Self {
            theme: config.settings.ui.theme,
            language: config.settings.ui.language.clone(),
            interval_minutes: config.settings.sync.interval_minutes,
            backfill_days: config.settings.sync.backfill_days,
            sync_all: config.settings.sync.sync_all_accessible,
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
    commits: Vec<Commit>,
    commits_total: u32,
    commits_repo: Option<i64>,
    charts: ChartsState,
    branches: Vec<Branch>,
    branches_repo: Option<i64>,
    branches_pending: Option<Receiver<Result<Vec<Branch>, GithubError>>>,
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
            commits: Vec::new(),
            commits_total: 0,
            commits_repo: None,
            charts: ChartsState::default(),
            branches: Vec::new(),
            branches_repo: None,
            branches_pending: None,
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
                Ok("charts") => {
                    app.repo_tab = RepoTab::Charts;
                    app.load_charts(repository.github_id);
                }
                Ok("diff") => {
                    app.repo_tab = RepoTab::Diff;
                    app.ensure_branches(&repository);
                    if std::env::var("GITPANEL_AUTO_DIFF").is_ok() {
                        app.start_diff();
                    }
                }
                _ => {}
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
                    self.select_default_branches();
                    self.branches_pending = None;
                }
                Ok(Err(error)) => {
                    self.branches_repo = None;
                    self.diff_error = Some(self.github_error_title(&error));
                    self.branches_pending = None;
                }
                Err(TryRecvError::Empty) => self.branches_pending = Some(receiver),
                Err(TryRecvError::Disconnected) => {
                    self.branches_repo = None;
                    self.branches_pending = None;
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
                self.load_commits(repo_id);
            }
            if self.charts.repo_id == Some(repo_id) {
                self.load_charts(repo_id);
            }
        }
    }

    fn load_commits(&mut self, repo_id: i64) {
        match self.db.list_commits(repo_id, 100, 0) {
            Ok(commits) => {
                self.commits = commits;
                self.commits_total = self.db.count_commits(repo_id).unwrap_or(0);
                self.commits_repo = Some(repo_id);
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

    fn load_more_commits(&mut self, repo_id: i64) {
        let offset = self.commits.len() as u32;
        match self.db.list_commits(repo_id, 100, offset) {
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
        match self.db.list_all_commits(repo_id) {
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
        self.selected_repo_id = Some(repo_id);
        self.screen = Screen::Repository;
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        self.repo_tab = RepoTab::Overview;
        self.repo_search.clear();
        self.commits.clear();
        self.commits_total = 0;
        self.commits_repo = None;
        self.charts = ChartsState::default();
        self.branches.clear();
        self.branches_repo = None;
        self.branches_pending = None;
        self.diff = None;
        self.diff_label.clear();
        self.diff_error = None;
        self.diff_loading = false;
        self.diff_pending = None;
        self.selected_commit = None;
        self.base_branch.clear();
        self.head_branch.clear();
        self.load_commits(repo_id);
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
        if self.branches_repo == Some(repository.github_id) || self.branches_pending.is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.branches_repo = Some(repository.github_id);
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
        self.settings = form.clone();
        self.config.settings.ui.theme = form.theme;
        self.config.settings.ui.language = form.language.clone();
        self.config.settings.sync.interval_minutes = form.interval_minutes;
        self.config.settings.sync.backfill_days = form.backfill_days;
        self.config.settings.sync.sync_all_accessible = form.sync_all;
        self.config.settings.normalize();
        if let Err(error) = self.config.save() {
            self.toast(
                format!("{}: {error}", self.translator.text(keys::ERROR_CONFIG)),
                ToastKind::Error,
            );
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
            });
        });
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
        } else if let Some(repo_id) = repo_table(
            ui,
            &filtered,
            &self.translator,
            palette,
            self.selected_repo_id,
            self.sync_state.current_repo_id,
        ) {
            self.open_repository(repo_id);
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
            self.translator.text(keys::TAB_COMMITS),
            self.translator.text(keys::TAB_CHARTS),
            self.translator.text(keys::TAB_DIFF),
        ];
        let tab_refs: Vec<&str> = tab_labels.iter().map(String::as_str).collect();
        let mut selected = self.repo_tab.index();
        if tabs(ui, &tab_refs, &mut selected, palette) {
            let tab = RepoTab::from_index(selected);
            self.repo_tab = tab;
            match tab {
                RepoTab::Commits => {
                    if self.commits_repo != Some(repository.github_id) {
                        self.load_commits(repository.github_id);
                    }
                }
                RepoTab::Charts => {
                    if self.charts.repo_id != Some(repository.github_id) {
                        self.load_charts(repository.github_id);
                    }
                }
                RepoTab::Diff => {
                    self.ensure_branches(&repository);
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
            RepoTab::Commits => self.commits_panel(ui, &repository),
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
            .commit_summary(repository.github_id)
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
        if self.commits.is_empty() {
            muted(ui, self.translator.text(keys::REPOS_NO_COMMITS), palette);
        } else {
            for commit in self.commits.iter().take(5) {
                commit_row(ui, commit, &self.translator, palette);
                ui.separator();
            }
        }
    }

    fn commits_panel(&mut self, ui: &mut Ui, repository: &Repository) {
        let palette = self.theme.palette;
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
            .id_salt("commits")
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
                self.load_more_commits(repository.github_id);
            }
        });
        if let Some(sha) = open_diff {
            self.open_commit_diff(sha);
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
            if self.commits_repo != Some(repository.github_id) {
                self.load_commits(repository.github_id);
            }
            if self.repo_tab == RepoTab::Charts && self.charts.repo_id != Some(repository.github_id)
            {
                self.load_charts(repository.github_id);
            }
            if self.repo_tab == RepoTab::Diff {
                self.ensure_branches(&repository);
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
                Screen::Repositories => self.screen_repositories(ui),
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
