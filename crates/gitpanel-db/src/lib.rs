#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use gitpanel_core::{Branch, Commit, CommitData, Repository};
use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS repos (
    github_id INTEGER PRIMARY KEY,
    owner TEXT NOT NULL,
    name TEXT NOT NULL,
    full_name TEXT NOT NULL UNIQUE,
    description TEXT,
    private INTEGER NOT NULL DEFAULT 0,
    visibility TEXT NOT NULL DEFAULT 'public',
    fork INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    default_branch TEXT NOT NULL DEFAULT 'main',
    language TEXT,
    stars INTEGER NOT NULL DEFAULT 0,
    permissions_admin INTEGER NOT NULL DEFAULT 0,
    html_url TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    pushed_at TEXT,
    synced_at TEXT,
    monitored INTEGER NOT NULL DEFAULT 1,
    last_sync_error TEXT
);

CREATE TABLE IF NOT EXISTS commits (
    repo_id INTEGER NOT NULL REFERENCES repos(github_id) ON DELETE CASCADE,
    sha TEXT NOT NULL,
    author_login TEXT,
    author_name TEXT NOT NULL,
    author_email TEXT,
    committed_at TEXT NOT NULL,
    message TEXT NOT NULL,
    additions INTEGER NOT NULL DEFAULT 0,
    deletions INTEGER NOT NULL DEFAULT 0,
    branch TEXT NOT NULL,
    is_merge INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (repo_id, sha)
);

CREATE INDEX IF NOT EXISTS commits_repo_date
    ON commits (repo_id, committed_at DESC);

CREATE INDEX IF NOT EXISTS commits_repo_author
    ON commits (repo_id, author_login);

CREATE TABLE IF NOT EXISTS branches (
    repo_id INTEGER NOT NULL REFERENCES repos(github_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sha TEXT NOT NULL,
    protected INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (repo_id, name)
);

CREATE TABLE IF NOT EXISTS diff_cache (
    repo_id INTEGER NOT NULL REFERENCES repos(github_id) ON DELETE CASCADE,
    base_sha TEXT NOT NULL,
    head_sha TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (repo_id, base_sha, head_sha)
);

CREATE TABLE IF NOT EXISTS content_cache (
    repo_id INTEGER NOT NULL REFERENCES repos(github_id) ON DELETE CASCADE,
    branch TEXT NOT NULL,
    path TEXT NOT NULL,
    payload TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (repo_id, branch, path)
);

CREATE TABLE IF NOT EXISTS release_cache (
    repo_id INTEGER PRIMARY KEY REFERENCES repos(github_id) ON DELETE CASCADE,
    payload TEXT NOT NULL,
    fetched_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS api_cache (
    repo_id INTEGER NOT NULL REFERENCES repos(github_id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    payload TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (repo_id, kind, key)
);

CREATE TABLE IF NOT EXISTS sync_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    repos INTEGER NOT NULL DEFAULT 0,
    commits INTEGER NOT NULL DEFAULT 0,
    error TEXT
);
"#;

#[derive(Clone)]
pub struct Db {
    path: Arc<PathBuf>,
}

impl Db {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, DbError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DbError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let db = Self {
            path: Arc::new(path),
        };
        let connection = db.connection()?;
        connection.execute_batch(SCHEMA_V1)?;
        connection.pragma_update(None, "user_version", 1)?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> Result<Connection, DbError> {
        let connection = Connection::open(self.path.as_ref())?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA synchronous = NORMAL;",
        )?;
        Ok(connection)
    }

    pub fn upsert_repositories(&self, repos: &[Repository]) -> Result<(), DbError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO repos (
                    github_id, owner, name, full_name, description, private, visibility,
                    fork, archived, default_branch, language, stars, permissions_admin,
                    html_url, updated_at, pushed_at, synced_at, monitored, last_sync_error
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                    ?14, ?15, ?16, ?17, ?18, ?19
                 )
                 ON CONFLICT(github_id) DO UPDATE SET
                    owner = excluded.owner,
                    name = excluded.name,
                    full_name = excluded.full_name,
                    description = excluded.description,
                    private = excluded.private,
                    visibility = excluded.visibility,
                    fork = excluded.fork,
                    archived = excluded.archived,
                    default_branch = excluded.default_branch,
                    language = excluded.language,
                    stars = excluded.stars,
                    permissions_admin = excluded.permissions_admin,
                    html_url = excluded.html_url,
                    updated_at = excluded.updated_at,
                    pushed_at = excluded.pushed_at",
            )?;
            for repo in repos {
                statement.execute(params![
                    repo.github_id,
                    repo.owner,
                    repo.name,
                    repo.full_name,
                    repo.description,
                    repo.private as i64,
                    repo.visibility,
                    repo.fork as i64,
                    repo.archived as i64,
                    repo.default_branch,
                    repo.language,
                    repo.stars as i64,
                    repo.permissions_admin as i64,
                    repo.html_url,
                    repo.updated_at.to_rfc3339(),
                    repo.pushed_at.map(|value| value.to_rfc3339()),
                    repo.synced_at.map(|value| value.to_rfc3339()),
                    repo.monitored as i64,
                    repo.last_sync_error,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_repositories(&self) -> Result<Vec<Repository>, DbError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT github_id, owner, name, full_name, description, private, visibility,
                    fork, archived, default_branch, language, stars, permissions_admin,
                    html_url, updated_at, pushed_at, synced_at, monitored, last_sync_error
             FROM repos
             ORDER BY COALESCE(pushed_at, updated_at) DESC, full_name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], row_to_repository)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    /// Remove cached repositories that were not present in the latest complete
    /// repository listing. Their commits and cached diffs disappear with them.
    pub fn remove_missing_repositories(&self, github_ids: &[i64]) -> Result<(), DbError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "CREATE TEMP TABLE IF NOT EXISTS keep_repositories (id INTEGER PRIMARY KEY)",
            [],
        )?;
        transaction.execute("DELETE FROM keep_repositories", [])?;
        {
            let mut statement =
                transaction.prepare("INSERT OR IGNORE INTO keep_repositories (id) VALUES (?1)")?;
            for github_id in github_ids {
                statement.execute([github_id])?;
            }
        }
        transaction.execute(
            "DELETE FROM repos WHERE github_id NOT IN (SELECT id FROM keep_repositories)",
            [],
        )?;
        transaction.execute("DROP TABLE keep_repositories", [])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_repository(&self, github_id: i64) -> Result<Option<Repository>, DbError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT github_id, owner, name, full_name, description, private, visibility,
                    fork, archived, default_branch, language, stars, permissions_admin,
                    html_url, updated_at, pushed_at, synced_at, monitored, last_sync_error
             FROM repos WHERE github_id = ?1",
        )?;
        statement
            .query_row([github_id], row_to_repository)
            .optional()
            .map_err(DbError::from)
    }

    pub fn set_repo_synced(&self, github_id: i64, at: DateTime<Utc>) -> Result<(), DbError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE repos SET synced_at = ?2, last_sync_error = NULL WHERE github_id = ?1",
            params![github_id, at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn set_repo_error(&self, github_id: i64, message: &str) -> Result<(), DbError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE repos SET last_sync_error = ?2 WHERE github_id = ?1",
            params![github_id, message],
        )?;
        Ok(())
    }

    pub fn upsert_commits(
        &self,
        repo_id: i64,
        branch: &str,
        commits: &[CommitData],
    ) -> Result<(), DbError> {
        if commits.is_empty() {
            return Ok(());
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO commits (
                    repo_id, sha, author_login, author_name, author_email, committed_at,
                    message, additions, deletions, branch, is_merge
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(repo_id, sha) DO UPDATE SET
                    author_login = excluded.author_login,
                    author_name = excluded.author_name,
                    author_email = excluded.author_email,
                    committed_at = excluded.committed_at,
                    message = excluded.message,
                    additions = excluded.additions,
                    deletions = excluded.deletions,
                    branch = excluded.branch,
                    is_merge = excluded.is_merge",
            )?;
            for commit in commits {
                statement.execute(params![
                    repo_id,
                    commit.sha,
                    commit.author_login,
                    commit.author_name,
                    commit.author_email,
                    commit.committed_at.to_rfc3339(),
                    commit.message,
                    commit.additions as i64,
                    commit.deletions as i64,
                    branch,
                    commit.is_merge as i64,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_commits(
        &self,
        repo_id: i64,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Commit>, DbError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT repo_id, sha, author_login, author_name, author_email, committed_at,
                    message, additions, deletions, branch, is_merge
             FROM commits
             WHERE repo_id = ?1
             ORDER BY committed_at DESC, sha
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows =
            statement.query_map(params![repo_id, limit as i64, offset as i64], row_to_commit)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn list_commits_for_branch(
        &self,
        repo_id: i64,
        branch: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Commit>, DbError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT repo_id, sha, author_login, author_name, author_email, committed_at,
                    message, additions, deletions, branch, is_merge
             FROM commits
             WHERE repo_id = ?1 AND branch = ?2
             ORDER BY committed_at DESC, sha
             LIMIT ?3 OFFSET ?4",
        )?;
        let rows = statement.query_map(
            params![repo_id, branch, limit as i64, offset as i64],
            row_to_commit,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn list_all_commits_for_branch(
        &self,
        repo_id: i64,
        branch: &str,
    ) -> Result<Vec<Commit>, DbError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT repo_id, sha, author_login, author_name, author_email, committed_at,
                    message, additions, deletions, branch, is_merge
             FROM commits
             WHERE repo_id = ?1 AND branch = ?2
             ORDER BY committed_at ASC, sha",
        )?;
        let rows = statement.query_map(params![repo_id, branch], row_to_commit)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn count_commits_for_branch(&self, repo_id: i64, branch: &str) -> Result<u32, DbError> {
        let connection = self.connection()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM commits WHERE repo_id = ?1 AND branch = ?2",
            params![repo_id, branch],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u32)
    }

    pub fn commit_summary_for_branch(
        &self,
        repo_id: i64,
        branch: &str,
    ) -> Result<(u32, u64, u64), DbError> {
        let connection = self.connection()?;
        let (count, additions, deletions) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(additions), 0), COALESCE(SUM(deletions), 0)
             FROM commits WHERE repo_id = ?1 AND branch = ?2",
            params![repo_id, branch],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        Ok((
            count.max(0) as u32,
            additions.max(0) as u64,
            deletions.max(0) as u64,
        ))
    }

    pub fn list_all_commits(&self, repo_id: i64) -> Result<Vec<Commit>, DbError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT repo_id, sha, author_login, author_name, author_email, committed_at,
                    message, additions, deletions, branch, is_merge
             FROM commits
             WHERE repo_id = ?1
             ORDER BY committed_at ASC, sha",
        )?;
        let rows = statement.query_map([repo_id], row_to_commit)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn count_commits(&self, repo_id: i64) -> Result<u32, DbError> {
        let connection = self.connection()?;
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM commits WHERE repo_id = ?1",
            [repo_id],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u32)
    }

    pub fn commit_summary(&self, repo_id: i64) -> Result<(u32, u64, u64), DbError> {
        let connection = self.connection()?;
        let (count, additions, deletions) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(additions), 0), COALESCE(SUM(deletions), 0)
             FROM commits WHERE repo_id = ?1",
            [repo_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )?;
        Ok((
            count.max(0) as u32,
            additions.max(0) as u64,
            deletions.max(0) as u64,
        ))
    }

    pub fn replace_branches(&self, repo_id: i64, branches: &[Branch]) -> Result<(), DbError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM branches WHERE repo_id = ?1", [repo_id])?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO branches (repo_id, name, sha, protected)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for branch in branches {
                statement.execute(params![
                    repo_id,
                    branch.name,
                    branch.sha,
                    branch.protected as i64,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_branches(&self, repo_id: i64) -> Result<Vec<Branch>, DbError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT name, sha, protected FROM branches WHERE repo_id = ?1 ORDER BY name",
        )?;
        let rows = statement.query_map([repo_id], |row| {
            Ok(Branch {
                name: row.get(0)?,
                sha: row.get(1)?,
                protected: row.get::<_, i64>(2)? != 0,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn get_diff_cache(
        &self,
        repo_id: i64,
        base_sha: &str,
        head_sha: &str,
    ) -> Result<Option<String>, DbError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT payload FROM diff_cache
                 WHERE repo_id = ?1 AND base_sha = ?2 AND head_sha = ?3",
                params![repo_id, base_sha, head_sha],
                |row| row.get(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn put_diff_cache(
        &self,
        repo_id: i64,
        base_sha: &str,
        head_sha: &str,
        payload: &str,
    ) -> Result<(), DbError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO diff_cache (repo_id, base_sha, head_sha, payload, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(repo_id, base_sha, head_sha) DO UPDATE SET
                payload = excluded.payload,
                created_at = excluded.created_at",
            params![
                repo_id,
                base_sha,
                head_sha,
                payload,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_content_cache(
        &self,
        repo_id: i64,
        branch: &str,
        path: &str,
    ) -> Result<Option<String>, DbError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT payload FROM content_cache
                 WHERE repo_id = ?1 AND branch = ?2 AND path = ?3",
                params![repo_id, branch, path],
                |row| row.get(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn put_content_cache(
        &self,
        repo_id: i64,
        branch: &str,
        path: &str,
        payload: &str,
    ) -> Result<(), DbError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO content_cache (repo_id, branch, path, payload, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(repo_id, branch, path) DO UPDATE SET
                payload = excluded.payload,
                fetched_at = excluded.fetched_at",
            params![repo_id, branch, path, payload, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn get_release_cache(&self, repo_id: i64) -> Result<Option<String>, DbError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT payload FROM release_cache WHERE repo_id = ?1",
                [repo_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn put_release_cache(&self, repo_id: i64, payload: &str) -> Result<(), DbError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO release_cache (repo_id, payload, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(repo_id) DO UPDATE SET
                payload = excluded.payload,
                fetched_at = excluded.fetched_at",
            params![repo_id, payload, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn get_api_cache(
        &self,
        repo_id: i64,
        kind: &str,
        key: &str,
    ) -> Result<Option<String>, DbError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT payload FROM api_cache
                 WHERE repo_id = ?1 AND kind = ?2 AND key = ?3",
                params![repo_id, kind, key],
                |row| row.get(0),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn put_api_cache(
        &self,
        repo_id: i64,
        kind: &str,
        key: &str,
        payload: &str,
    ) -> Result<(), DbError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO api_cache (repo_id, kind, key, payload, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(repo_id, kind, key) DO UPDATE SET
                payload = excluded.payload,
                fetched_at = excluded.fetched_at",
            params![repo_id, kind, key, payload, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn start_sync_run(&self, at: DateTime<Utc>) -> Result<i64, DbError> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO sync_runs (started_at, status) VALUES (?1, 'running')",
            [at.to_rfc3339()],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn finish_sync_run(
        &self,
        id: i64,
        status: &str,
        repos: u32,
        commits: u32,
        error: Option<&str>,
    ) -> Result<(), DbError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE sync_runs
             SET finished_at = ?2, status = ?3, repos = ?4, commits = ?5, error = ?6
             WHERE id = ?1",
            params![
                id,
                Utc::now().to_rfc3339(),
                status,
                repos as i64,
                commits as i64,
                error
            ],
        )?;
        Ok(())
    }
}

fn row_to_repository(row: &rusqlite::Row<'_>) -> rusqlite::Result<Repository> {
    Ok(Repository {
        github_id: row.get(0)?,
        owner: row.get(1)?,
        name: row.get(2)?,
        full_name: row.get(3)?,
        description: row.get(4)?,
        private: row.get::<_, i64>(5)? != 0,
        visibility: row.get(6)?,
        fork: row.get::<_, i64>(7)? != 0,
        archived: row.get::<_, i64>(8)? != 0,
        default_branch: row.get(9)?,
        language: row.get(10)?,
        stars: row.get::<_, i64>(11)?.max(0) as u32,
        permissions_admin: row.get::<_, i64>(12)? != 0,
        html_url: row.get(13)?,
        updated_at: parse_required_datetime(row.get::<_, String>(14)?)?,
        pushed_at: row
            .get::<_, Option<String>>(15)?
            .map(parse_required_datetime)
            .transpose()?,
        synced_at: row
            .get::<_, Option<String>>(16)?
            .map(parse_required_datetime)
            .transpose()?,
        monitored: row.get::<_, i64>(17)? != 0,
        last_sync_error: row.get(18)?,
    })
}

fn row_to_commit(row: &rusqlite::Row<'_>) -> rusqlite::Result<Commit> {
    Ok(Commit {
        repo_id: row.get(0)?,
        sha: row.get(1)?,
        author_login: row.get(2)?,
        author_name: row.get(3)?,
        author_email: row.get(4)?,
        committed_at: parse_required_datetime(row.get::<_, String>(5)?)?,
        message: row.get(6)?,
        additions: row.get::<_, i64>(7)?.max(0) as u32,
        deletions: row.get::<_, i64>(8)?.max(0) as u32,
        branch: row.get(9)?,
        is_merge: row.get::<_, i64>(10)? != 0,
    })
}

fn parse_required_datetime(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|date| date.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn repo() -> Repository {
        Repository {
            github_id: 1,
            owner: "MiguelSilvaPorto".to_owned(),
            name: "gitpanel".to_owned(),
            full_name: "MiguelSilvaPorto/gitpanel".to_owned(),
            description: Some("Painel".to_owned()),
            private: false,
            visibility: "public".to_owned(),
            fork: false,
            archived: false,
            default_branch: "main".to_owned(),
            language: Some("Rust".to_owned()),
            stars: 3,
            permissions_admin: true,
            html_url: "https://github.com/MiguelSilvaPorto/gitpanel".to_owned(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            pushed_at: Some(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap()),
            synced_at: None,
            monitored: true,
            last_sync_error: None,
        }
    }

    fn commit(sha: &str) -> CommitData {
        CommitData {
            sha: sha.to_owned(),
            author_login: Some("miguel".to_owned()),
            author_name: "Miguel".to_owned(),
            author_email: Some("miguel@example.com".to_owned()),
            committed_at: Utc.with_ymd_and_hms(2026, 1, 3, 12, 0, 0).unwrap(),
            message: "feat: initial".to_owned(),
            additions: 10,
            deletions: 2,
            is_merge: false,
        }
    }

    #[test]
    fn stores_and_reads_repositories_and_commits() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = Db::open(temp.path().join("test.db")).expect("db");
        db.upsert_repositories(&[repo()]).expect("repo");
        let repos = db.list_repositories().expect("list");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].full_name, "MiguelSilvaPorto/gitpanel");

        db.upsert_commits(1, "main", &[commit("abc")])
            .expect("commit");
        db.upsert_commits(1, "main", &[commit("abc")])
            .expect("dedupe");
        assert_eq!(db.count_commits(1).expect("count"), 1);
        assert_eq!(db.commit_summary(1).expect("summary"), (1, 10, 2));
        let commits = db.list_commits(1, 10, 0).expect("commits");
        assert_eq!(commits[0].sha, "abc");
    }

    #[test]
    fn caches_diff_and_branches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = Db::open(temp.path().join("test.db")).expect("db");
        db.upsert_repositories(&[repo()]).expect("repo");
        db.replace_branches(
            1,
            &[Branch {
                name: "main".to_owned(),
                sha: "abc".to_owned(),
                protected: true,
            }],
        )
        .expect("branches");
        assert_eq!(db.list_branches(1).expect("branches").len(), 1);
        db.put_diff_cache(1, "abc", "def", "{}").expect("put");
        assert_eq!(
            db.get_diff_cache(1, "abc", "def").expect("get"),
            Some("{}".to_owned())
        );
    }

    #[test]
    fn removes_repositories_missing_from_the_latest_listing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = Db::open(temp.path().join("test.db")).expect("db");
        let mut second = repo();
        second.github_id = 2;
        second.name = "other".to_owned();
        second.full_name = "MiguelSilvaPorto/other".to_owned();
        db.upsert_repositories(&[repo(), second]).expect("repos");
        db.upsert_commits(2, "main", &[commit("other-commit")])
            .expect("commit");
        db.remove_missing_repositories(&[1]).expect("cleanup");
        let repos = db.list_repositories().expect("repos");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].github_id, 1);
        assert_eq!(db.count_commits(2).expect("count"), 0);
    }

    #[test]
    fn caches_branch_commits_contents_and_releases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let db = Db::open(temp.path().join("test.db")).expect("db");
        db.upsert_repositories(&[repo()]).expect("repo");
        db.upsert_commits(1, "main", &[commit("main-commit")])
            .expect("main");
        db.upsert_commits(1, "feature", &[commit("feature-commit")])
            .expect("feature");
        assert_eq!(
            db.count_commits_for_branch(1, "main").expect("main count"),
            1
        );
        assert_eq!(
            db.list_commits_for_branch(1, "feature", 10, 0)
                .expect("feature list")[0]
                .sha,
            "feature-commit"
        );
        db.put_content_cache(1, "main", "", "{\"kind\":\"dir\"}")
            .expect("contents");
        assert_eq!(
            db.get_content_cache(1, "main", "").expect("contents get"),
            Some("{\"kind\":\"dir\"}".to_owned())
        );
        db.put_release_cache(1, "[]").expect("releases");
        assert_eq!(
            db.get_release_cache(1).expect("releases get"),
            Some("[]".to_owned())
        );
        db.put_api_cache(1, "issues", "open", "[]")
            .expect("api cache");
        assert_eq!(
            db.get_api_cache(1, "issues", "open").expect("api get"),
            Some("[]".to_owned())
        );
    }
}
