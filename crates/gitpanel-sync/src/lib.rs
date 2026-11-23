#![forbid(unsafe_code)]

use std::thread;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use gitpanel_config::SyncSettings;
use gitpanel_core::Repository;
use gitpanel_db::Db;
use gitpanel_github::{GithubClient, GithubError};
use thiserror::Error;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Debug, Clone)]
pub enum SyncCommand {
    SyncAll,
    SyncRepo(i64),
    Pause,
    Resume,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum SyncEvent {
    LoadingRepositories,
    RepositoriesUpdated,
    Started {
        total: usize,
    },
    RepoStarted {
        repo_id: i64,
        full_name: String,
    },
    RepoFinished {
        repo_id: i64,
        added: u32,
        error: Option<String>,
    },
    Progress {
        current: usize,
        total: usize,
    },
    RateLimit {
        limit: u32,
        remaining: u32,
        reset_at: DateTime<Utc>,
    },
    RateLimited {
        reset_at: DateTime<Utc>,
    },
    Finished {
        total: usize,
        added: u32,
        errors: usize,
    },
    Paused(bool),
    Error {
        code: String,
        message: String,
    },
}

pub struct SyncHandle {
    commands: UnboundedSender<SyncCommand>,
    events: Receiver<SyncEvent>,
    worker: Option<thread::JoinHandle<()>>,
}

impl SyncHandle {
    pub fn spawn(db: Db, client: GithubClient, settings: SyncSettings) -> Result<Self, SyncError> {
        let (commands, command_rx) = unbounded_channel();
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let worker = thread::Builder::new()
            .name("gitpanel-sync".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build();
                let runtime = match runtime {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = event_tx.send(SyncEvent::Error {
                            code: "error-sync".to_owned(),
                            message: error.to_string(),
                        });
                        return;
                    }
                };
                runtime.block_on(run_worker(db, client, settings, command_rx, event_tx));
            })?;
        Ok(Self {
            commands,
            events: event_rx,
            worker: Some(worker),
        })
    }

    pub fn sync_all(&self) -> Result<(), SyncError> {
        self.commands
            .send(SyncCommand::SyncAll)
            .map_err(|_| SyncError::Stopped)
    }

    pub fn sync_repo(&self, repo_id: i64) -> Result<(), SyncError> {
        self.commands
            .send(SyncCommand::SyncRepo(repo_id))
            .map_err(|_| SyncError::Stopped)
    }

    pub fn pause(&self) -> Result<(), SyncError> {
        self.commands
            .send(SyncCommand::Pause)
            .map_err(|_| SyncError::Stopped)
    }

    pub fn resume(&self) -> Result<(), SyncError> {
        self.commands
            .send(SyncCommand::Resume)
            .map_err(|_| SyncError::Stopped)
    }

    pub fn shutdown(&mut self) {
        let _ = self.commands.send(SyncCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }

    pub fn try_recv_events(&self) -> Vec<SyncEvent> {
        let mut events = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        events
    }
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(SyncCommand::Shutdown);
    }
}

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("could not start sync worker: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("sync worker is not running")]
    Stopped,
}

#[derive(Debug, Default)]
struct SyncOutcome {
    total: usize,
    added: u32,
    errors: usize,
    rate_limited_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct OneOutcome {
    added: u32,
    rate_limited_until: Option<DateTime<Utc>>,
    incomplete: bool,
}

#[derive(Debug, Error)]
enum SyncFailure {
    #[error(transparent)]
    Github(#[from] GithubError),
    #[error(transparent)]
    Db(#[from] gitpanel_db::DbError),
}

async fn run_worker(
    db: Db,
    client: GithubClient,
    settings: SyncSettings,
    mut commands: UnboundedReceiver<SyncCommand>,
    events: Sender<SyncEvent>,
) {
    let interval = Duration::from_secs(settings.interval_minutes.max(1).saturating_mul(60));
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut paused = false;
    let mut paused_until: Option<DateTime<Utc>> = None;

    loop {
        if let Some(until) = paused_until {
            if Utc::now() >= until {
                paused_until = None;
            } else {
                let remaining = (until - Utc::now()).to_std().unwrap_or(Duration::ZERO);
                tokio::select! {
                    command = commands.recv() => {
                        if handle_command(command, &mut paused, &mut paused_until, &db, &client, &settings, &events).await {
                            break;
                        }
                    }
                    _ = tokio::time::sleep(remaining) => {}
                }
                continue;
            }
        }

        tokio::select! {
            command = commands.recv() => {
                if handle_command(command, &mut paused, &mut paused_until, &db, &client, &settings, &events).await {
                    break;
                }
            }
            _ = ticker.tick(), if !paused => {
                let outcome = sync_all(&db, &client, &settings, &events).await;
                if let Some(until) = outcome.rate_limited_until {
                    paused_until = Some(until);
                    let _ = events.send(SyncEvent::RateLimited { reset_at: until });
                }
            }
        }
    }
}

async fn handle_command(
    command: Option<SyncCommand>,
    paused: &mut bool,
    paused_until: &mut Option<DateTime<Utc>>,
    db: &Db,
    client: &GithubClient,
    settings: &SyncSettings,
    events: &Sender<SyncEvent>,
) -> bool {
    match command {
        Some(SyncCommand::SyncAll) => {
            *paused_until = None;
            let outcome = sync_all(db, client, settings, events).await;
            if let Some(until) = outcome.rate_limited_until {
                *paused_until = Some(until);
                let _ = events.send(SyncEvent::RateLimited { reset_at: until });
            }
        }
        Some(SyncCommand::SyncRepo(repo_id)) => {
            let _ = sync_single(db, client, settings, repo_id, events).await;
        }
        Some(SyncCommand::Pause) => {
            *paused = true;
            let _ = events.send(SyncEvent::Paused(true));
        }
        Some(SyncCommand::Resume) => {
            *paused = false;
            *paused_until = None;
            let _ = events.send(SyncEvent::Paused(false));
        }
        Some(SyncCommand::Shutdown) | None => return true,
    }
    false
}

async fn sync_all(
    db: &Db,
    client: &GithubClient,
    settings: &SyncSettings,
    events: &Sender<SyncEvent>,
) -> SyncOutcome {
    let mut outcome = SyncOutcome::default();
    let _ = events.send(SyncEvent::LoadingRepositories);
    let repositories = match client.list_repositories().await {
        Ok(repositories) => repositories,
        Err(error) => {
            if let GithubError::RateLimited { reset_at, .. } = error {
                outcome.rate_limited_until = Some(reset_at);
            }
            let _ = events.send(SyncEvent::Error {
                code: error.code().to_owned(),
                message: error.to_string(),
            });
            return outcome;
        }
    };
    if let Err(error) = db.upsert_repositories(&repositories) {
        let _ = events.send(SyncEvent::Error {
            code: "error-db".to_owned(),
            message: error.to_string(),
        });
        return outcome;
    }
    let ids: Vec<i64> = repositories.iter().map(|repo| repo.github_id).collect();
    if let Err(error) = db.remove_missing_repositories(&ids) {
        tracing::warn!(%error, "could not remove stale repositories");
    }
    let _ = events.send(SyncEvent::RepositoriesUpdated);

    let repositories = match db.list_repositories() {
        Ok(repositories) => repositories,
        Err(error) => {
            let _ = events.send(SyncEvent::Error {
                code: "error-db".to_owned(),
                message: error.to_string(),
            });
            return outcome;
        }
    };
    outcome.total = repositories.len();
    let _ = events.send(SyncEvent::Started {
        total: outcome.total,
    });

    for (index, repo) in repositories.iter().enumerate() {
        let _ = events.send(SyncEvent::RepoStarted {
            repo_id: repo.github_id,
            full_name: repo.full_name.clone(),
        });
        match sync_one(db, client, settings, repo, events).await {
            Ok(one) => {
                outcome.added = outcome.added.saturating_add(one.added);
                if one.rate_limited_until.is_some() {
                    outcome.rate_limited_until = one.rate_limited_until;
                }
                if one.incomplete {
                    let _ = db.set_repo_error(
                        repo.github_id,
                        "historico maior que o limite desta sincronizacao",
                    );
                }
                let _ = events.send(SyncEvent::RepoFinished {
                    repo_id: repo.github_id,
                    added: one.added,
                    error: None,
                });
                if outcome.rate_limited_until.is_some() {
                    break;
                }
            }
            Err(SyncFailure::Github(error)) if error.is_rate_limited() => {
                if let GithubError::RateLimited { reset_at, .. } = error {
                    outcome.rate_limited_until = Some(reset_at);
                }
                outcome.errors += 1;
                let _ = db.set_repo_error(repo.github_id, &error.to_string());
                let _ = events.send(SyncEvent::RepoFinished {
                    repo_id: repo.github_id,
                    added: 0,
                    error: Some(error.to_string()),
                });
                break;
            }
            Err(error) => {
                outcome.errors += 1;
                let _ = db.set_repo_error(repo.github_id, &error.to_string());
                let _ = events.send(SyncEvent::RepoFinished {
                    repo_id: repo.github_id,
                    added: 0,
                    error: Some(error.to_string()),
                });
            }
        }
        let _ = events.send(SyncEvent::Progress {
            current: index + 1,
            total: outcome.total,
        });
    }

    let _ = events.send(SyncEvent::Finished {
        total: outcome.total,
        added: outcome.added,
        errors: outcome.errors,
    });
    outcome
}

async fn sync_single(
    db: &Db,
    client: &GithubClient,
    settings: &SyncSettings,
    repo_id: i64,
    events: &Sender<SyncEvent>,
) {
    let repo = match db.get_repository(repo_id) {
        Ok(Some(repo)) => repo,
        Ok(None) => {
            let _ = events.send(SyncEvent::Error {
                code: "error-not-found".to_owned(),
                message: "repository not found in local database".to_owned(),
            });
            return;
        }
        Err(error) => {
            let _ = events.send(SyncEvent::Error {
                code: "error-db".to_owned(),
                message: error.to_string(),
            });
            return;
        }
    };
    let _ = events.send(SyncEvent::Started { total: 1 });
    let _ = events.send(SyncEvent::RepoStarted {
        repo_id: repo.github_id,
        full_name: repo.full_name.clone(),
    });
    match sync_one(db, client, settings, &repo, events).await {
        Ok(one) => {
            let _ = events.send(SyncEvent::RepoFinished {
                repo_id: repo.github_id,
                added: one.added,
                error: None,
            });
            if let Some(until) = one.rate_limited_until {
                let _ = events.send(SyncEvent::RateLimited { reset_at: until });
            }
            let _ = events.send(SyncEvent::Finished {
                total: 1,
                added: one.added,
                errors: 0,
            });
        }
        Err(error) => {
            let _ = db.set_repo_error(repo.github_id, &error.to_string());
            let _ = events.send(SyncEvent::RepoFinished {
                repo_id: repo.github_id,
                added: 0,
                error: Some(error.to_string()),
            });
            let _ = events.send(SyncEvent::Finished {
                total: 1,
                added: 0,
                errors: 1,
            });
        }
    }
    let _ = events.send(SyncEvent::Progress {
        current: 1,
        total: 1,
    });
}

async fn sync_one(
    db: &Db,
    client: &GithubClient,
    settings: &SyncSettings,
    repo: &Repository,
    events: &Sender<SyncEvent>,
) -> Result<OneOutcome, SyncFailure> {
    if !needs_sync(repo) {
        return Ok(OneOutcome::default());
    }
    let since = sync_since(repo, Utc::now(), settings.backfill_days);
    let mut cursor: Option<String> = None;
    let mut added = 0u32;
    let mut pages = 0usize;
    let mut incomplete = false;
    let mut rate_limited_until = None;

    loop {
        let page = client
            .fetch_commits_page(
                &repo.owner,
                &repo.name,
                &repo.default_branch,
                since,
                cursor.as_deref(),
            )
            .await?;
        let mut rate_limited = false;
        if let Some(rate) = page.rate_limit {
            let _ = events.send(SyncEvent::RateLimit {
                limit: rate.limit,
                remaining: rate.remaining,
                reset_at: rate.reset_at,
            });
            if rate.remaining == 0 {
                rate_limited_until = Some(rate.reset_at);
                rate_limited = true;
            }
        }
        let page_len = page.commits.len() as u32;
        db.upsert_commits(repo.github_id, &repo.default_branch, &page.commits)?;
        added = added.saturating_add(page_len);
        pages += 1;
        if rate_limited {
            incomplete = page.has_next;
            break;
        }
        if !page.has_next {
            break;
        }
        if pages >= 200 {
            incomplete = true;
            break;
        }
        cursor = page.end_cursor;
        if cursor.is_none() {
            incomplete = true;
            break;
        }
    }

    if !incomplete {
        db.set_repo_synced(repo.github_id, Utc::now())?;
    }
    Ok(OneOutcome {
        added,
        rate_limited_until,
        incomplete,
    })
}

pub fn needs_sync(repo: &Repository) -> bool {
    if repo.default_branch.is_empty() {
        return false;
    }
    if repo.last_sync_error.is_some() {
        return true;
    }
    let Some(synced_at) = repo.synced_at else {
        return true;
    };
    repo.pushed_at.is_none_or(|pushed_at| pushed_at > synced_at)
}

pub fn sync_since(repo: &Repository, now: DateTime<Utc>, backfill_days: u32) -> DateTime<Utc> {
    if let Some(synced_at) = repo.synced_at {
        return synced_at - TimeDelta::minutes(5);
    }
    now - TimeDelta::days(i64::from(backfill_days.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn repo(synced_at: Option<DateTime<Utc>>, pushed_at: Option<DateTime<Utc>>) -> Repository {
        Repository {
            github_id: 1,
            owner: "owner".to_owned(),
            name: "repo".to_owned(),
            full_name: "owner/repo".to_owned(),
            description: None,
            private: false,
            visibility: "public".to_owned(),
            fork: false,
            archived: false,
            default_branch: "main".to_owned(),
            language: None,
            stars: 0,
            permissions_admin: false,
            html_url: "https://github.com/owner/repo".to_owned(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            pushed_at,
            synced_at,
            monitored: true,
            last_sync_error: None,
        }
    }

    #[test]
    fn decides_when_to_sync() {
        assert!(needs_sync(&repo(None, None)));
        let synced = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap();
        let pushed = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        assert!(!needs_sync(&repo(Some(synced), Some(pushed))));
        assert!(needs_sync(&repo(Some(pushed), Some(synced))));
    }

    #[test]
    fn incremental_sync_overlaps_five_minutes() {
        let synced = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 1, 3, 0, 0, 0).unwrap();
        let since = sync_since(&repo(Some(synced), None), now, 90);
        assert_eq!(since, synced - TimeDelta::minutes(5));
        let initial = sync_since(&repo(None, None), now, 90);
        assert_eq!(initial, now - TimeDelta::days(90));
    }
}
