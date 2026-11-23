#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike,
    Utc,
};
use gitpanel_core::Commit;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Granularity {
    Minute,
    Hour,
    Day,
    Week,
    Month,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitPoint {
    pub start: DateTime<Utc>,
    pub count: u32,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub commits: u32,
    pub additions: u64,
    pub deletions: u64,
    pub authors: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributorStat {
    pub login: String,
    pub name: String,
    pub commits: u32,
    pub additions: u64,
    pub deletions: u64,
}

pub fn aggregate_local(
    commits: &[Commit],
    granularity: Granularity,
    include_merges: bool,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> Vec<CommitPoint> {
    aggregate_with_tz(commits, granularity, include_merges, since, until, &Local)
}

pub fn aggregate_with_offset(
    commits: &[Commit],
    granularity: Granularity,
    include_merges: bool,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    offset: FixedOffset,
) -> Vec<CommitPoint> {
    aggregate_with_tz(commits, granularity, include_merges, since, until, &offset)
}

fn aggregate_with_tz<Tz>(
    commits: &[Commit],
    granularity: Granularity,
    include_merges: bool,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    timezone: &Tz,
) -> Vec<CommitPoint>
where
    Tz: TimeZone,
    Tz::Offset: Copy,
{
    let mut buckets: BTreeMap<NaiveDateTime, CommitPoint> = BTreeMap::new();
    for commit in commits {
        if !include_merges && commit.is_merge {
            continue;
        }
        if since.is_some_and(|start| commit.committed_at < start) {
            continue;
        }
        if until.is_some_and(|end| commit.committed_at >= end) {
            continue;
        }
        let local = commit.committed_at.with_timezone(timezone);
        let Some(bucket) = bucket_start(local.naive_local(), granularity) else {
            continue;
        };
        let start = timezone
            .from_local_datetime(&bucket)
            .earliest()
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or(commit.committed_at);
        let point = buckets.entry(bucket).or_insert(CommitPoint {
            start,
            count: 0,
            additions: 0,
            deletions: 0,
        });
        point.count += 1;
        point.additions += u64::from(commit.additions);
        point.deletions += u64::from(commit.deletions);
    }
    buckets.into_values().collect()
}

pub fn summary(commits: &[Commit], include_merges: bool) -> Summary {
    let mut authors = std::collections::HashSet::new();
    let mut result = Summary::default();
    for commit in commits {
        if !include_merges && commit.is_merge {
            continue;
        }
        result.commits += 1;
        result.additions += u64::from(commit.additions);
        result.deletions += u64::from(commit.deletions);
        authors.insert(commit.author_display().to_owned());
    }
    result.authors = authors.len() as u32;
    result
}

pub fn top_contributors(
    commits: &[Commit],
    include_merges: bool,
    limit: usize,
) -> Vec<ContributorStat> {
    let mut contributors: BTreeMap<String, ContributorStat> = BTreeMap::new();
    for commit in commits {
        if !include_merges && commit.is_merge {
            continue;
        }
        let login = commit.author_display().to_owned();
        let entry = contributors
            .entry(login.clone())
            .or_insert_with(|| ContributorStat {
                login,
                name: commit.author_name.clone(),
                commits: 0,
                additions: 0,
                deletions: 0,
            });
        entry.commits += 1;
        entry.additions += u64::from(commit.additions);
        entry.deletions += u64::from(commit.deletions);
    }
    let mut values: Vec<_> = contributors.into_values().collect();
    values.sort_by(|left, right| {
        right
            .commits
            .cmp(&left.commits)
            .then_with(|| right.additions.cmp(&left.additions))
    });
    values.truncate(limit);
    values
}

pub fn period_start(now: DateTime<Utc>, days: u32) -> Option<DateTime<Utc>> {
    if days == 0 {
        None
    } else {
        now.checked_sub_signed(Duration::days(i64::from(days)))
    }
}

fn bucket_start(value: NaiveDateTime, granularity: Granularity) -> Option<NaiveDateTime> {
    let date = value.date();
    let time = value.time();
    match granularity {
        Granularity::Minute => date.and_hms_opt(time.hour(), time.minute(), 0),
        Granularity::Hour => date.and_hms_opt(time.hour(), 0, 0),
        Granularity::Day => date.and_hms_opt(0, 0, 0),
        Granularity::Week => {
            let monday = date.checked_sub_signed(Duration::days(i64::from(
                date.weekday().num_days_from_monday(),
            )))?;
            monday.and_hms_opt(0, 0, 0)
        }
        Granularity::Month => {
            let first = NaiveDate::from_ymd_opt(date.year(), date.month(), 1)?;
            first.and_hms_opt(0, 0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn commit(id: &str, at: DateTime<Utc>, merge: bool) -> Commit {
        Commit {
            repo_id: 1,
            sha: id.to_owned(),
            author_login: Some("miguel".to_owned()),
            author_name: "Miguel".to_owned(),
            author_email: None,
            committed_at: at,
            message: "commit".to_owned(),
            additions: 3,
            deletions: 1,
            branch: "main".to_owned(),
            is_merge: merge,
        }
    }

    #[test]
    fn aggregates_by_day_in_utc() {
        let offset = FixedOffset::east_opt(0).unwrap();
        let commits = vec![
            commit(
                "a",
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap(),
                false,
            ),
            commit(
                "b",
                Utc.with_ymd_and_hms(2026, 1, 1, 11, 0, 0).unwrap(),
                false,
            ),
            commit(
                "c",
                Utc.with_ymd_and_hms(2026, 1, 2, 9, 0, 0).unwrap(),
                false,
            ),
        ];
        let points = aggregate_with_offset(&commits, Granularity::Day, true, None, None, offset);
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].count, 2);
        assert_eq!(points[0].additions, 6);
        assert_eq!(points[1].count, 1);
    }

    #[test]
    fn aggregates_by_minute_and_filters_merges() {
        let offset = FixedOffset::east_opt(0).unwrap();
        let commits = vec![
            commit(
                "a",
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 1).unwrap(),
                false,
            ),
            commit(
                "b",
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 30).unwrap(),
                false,
            ),
            commit(
                "c",
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 40).unwrap(),
                true,
            ),
        ];
        let points =
            aggregate_with_offset(&commits, Granularity::Minute, false, None, None, offset);
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].count, 2);
    }

    #[test]
    fn filters_period() {
        let offset = FixedOffset::east_opt(0).unwrap();
        let commits = vec![
            commit(
                "a",
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap(),
                false,
            ),
            commit(
                "b",
                Utc.with_ymd_and_hms(2026, 1, 2, 10, 0, 0).unwrap(),
                false,
            ),
        ];
        let points = aggregate_with_offset(
            &commits,
            Granularity::Day,
            true,
            Some(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap()),
            None,
            offset,
        );
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].count, 1);
    }

    #[test]
    fn groups_top_contributors() {
        let commits = vec![
            commit(
                "a",
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).unwrap(),
                false,
            ),
            commit(
                "b",
                Utc.with_ymd_and_hms(2026, 1, 1, 11, 0, 0).unwrap(),
                false,
            ),
        ];
        let top = top_contributors(&commits, true, 5);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].commits, 2);
        assert_eq!(summary(&commits, true).additions, 6);
    }
}
