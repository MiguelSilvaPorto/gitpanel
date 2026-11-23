use gitpanel_core::FilePatch;

use crate::model::{DiffFile, DiffHunk, DiffLine, DiffLineKind, FileStatus};

pub fn parse_file_patch(file: &FilePatch) -> DiffFile {
    let hunks = file.patch.as_deref().map(parse_hunks).unwrap_or_default();
    let truncated = file.patch.is_none() && (file.additions > 0 || file.deletions > 0);
    let binary = file.patch.is_none() && file.additions == 0 && file.deletions == 0;
    DiffFile {
        old_path: file
            .previous_filename
            .clone()
            .or_else(|| Some(file.filename.clone())),
        new_path: file.filename.clone(),
        status: FileStatus::from_github(&file.status),
        additions: file.additions,
        deletions: file.deletions,
        changes: file.changes,
        hunks,
        binary,
        truncated,
    }
}

pub fn parse_hunks(patch: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<DiffHunk> = None;
    let mut old_line = 0u32;
    let mut new_line = 0u32;

    for raw in patch.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            if let Some((old_start, old_lines, new_start, new_lines, context)) =
                parse_hunk_header(header)
            {
                old_line = old_start;
                new_line = new_start;
                current = Some(DiffHunk {
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    header: context,
                    lines: Vec::new(),
                });
            }
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };

        if line.starts_with('\\') {
            continue;
        }
        let (kind, content) = match line.chars().next() {
            Some('+') => (DiffLineKind::Added, &line[1..]),
            Some('-') => (DiffLineKind::Removed, &line[1..]),
            Some(' ') => (DiffLineKind::Context, &line[1..]),
            Some(_) => (DiffLineKind::Context, line),
            None => (DiffLineKind::Context, ""),
        };

        let (old_lineno, new_lineno) = match kind {
            DiffLineKind::Context => {
                let values = (Some(old_line), Some(new_line));
                old_line = old_line.saturating_add(1);
                new_line = new_line.saturating_add(1);
                values
            }
            DiffLineKind::Added => {
                let values = (None, Some(new_line));
                new_line = new_line.saturating_add(1);
                values
            }
            DiffLineKind::Removed => {
                let values = (Some(old_line), None);
                old_line = old_line.saturating_add(1);
                values
            }
        };

        hunk.lines.push(DiffLine {
            kind,
            content: content.to_owned(),
            old_lineno,
            new_lineno,
        });
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    hunks
}

fn parse_hunk_header(value: &str) -> Option<(u32, u32, u32, u32, String)> {
    let end = value.find(" @@").unwrap_or(value.len());
    let ranges = &value[..end];
    let mut parts = ranges.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let (old_start, old_lines) = parse_range(old)?;
    let (new_start, new_lines) = parse_range(new)?;
    let context = value[end..]
        .trim_start_matches(" @@")
        .trim_start()
        .to_owned();
    Some((old_start, old_lines, new_start, new_lines, context))
}

fn parse_range(value: &str) -> Option<(u32, u32)> {
    let mut parts = value.split(',');
    let start = parts.next()?.parse().ok()?;
    let count = parts.next().map(str::parse).transpose().ok()?.unwrap_or(1);
    Some((start, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(status: &str, text: Option<&str>, additions: u32, deletions: u32) -> FilePatch {
        FilePatch {
            filename: "src/main.rs".to_owned(),
            previous_filename: None,
            status: status.to_owned(),
            additions,
            deletions,
            changes: additions + deletions,
            patch: text.map(str::to_owned),
        }
    }

    #[test]
    fn parses_hunks_and_line_numbers() {
        let file = patch(
            "modified",
            Some("@@ -1,3 +1,4 @@\n fn main() {\n-    old();\n+    new();\n+    extra();\n }\n"),
            2,
            1,
        );
        let parsed = parse_file_patch(&file);
        assert_eq!(parsed.hunks.len(), 1);
        let lines = &parsed.hunks[0].lines;
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[1].kind, DiffLineKind::Removed);
        assert_eq!(lines[1].old_lineno, Some(2));
        assert_eq!(lines[1].new_lineno, None);
        assert_eq!(lines[2].kind, DiffLineKind::Added);
        assert_eq!(lines[2].old_lineno, None);
        assert_eq!(lines[2].new_lineno, Some(2));
        assert_eq!(lines[3].new_lineno, Some(3));
    }

    #[test]
    fn marks_binary_and_truncated() {
        let parsed = parse_file_patch(&patch("modified", None, 0, 0));
        assert!(parsed.binary);
        assert!(!parsed.truncated);
        let parsed = parse_file_patch(&patch("modified", None, 12, 3));
        assert!(!parsed.binary);
        assert!(parsed.truncated);
    }

    #[test]
    fn builds_side_by_side_rows() {
        let file = parse_file_patch(&patch(
            "modified",
            Some("@@ -1,2 +1,2 @@\n same();\n-old();\n+new();\n"),
            1,
            1,
        ));
        let views = file.side_by_side_hunks();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].rows.len(), 2);
        assert!(views[0].rows[0].0.is_some());
        assert!(views[0].rows[1].0.is_some());
        assert!(views[0].rows[1].1.is_some());
    }
}
