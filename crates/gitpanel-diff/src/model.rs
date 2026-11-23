use gitpanel_core::FilePatch;

use crate::parser::parse_file_patch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    Unknown,
}

impl FileStatus {
    pub fn from_github(status: &str) -> Self {
        match status {
            "added" => Self::Added,
            "removed" | "deleted" => Self::Deleted,
            "modified" | "changed" => Self::Modified,
            "renamed" => Self::Renamed,
            "copied" => Self::Copied,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diff {
    pub files: Vec<DiffFile>,
    pub additions: u64,
    pub deletions: u64,
}

impl Diff {
    pub fn from_patches(patches: &[FilePatch]) -> Self {
        let files: Vec<_> = patches.iter().map(parse_file_patch).collect();
        let additions = files.iter().map(|file| u64::from(file.additions)).sum();
        let deletions = files.iter().map(|file| u64::from(file.deletions)).sum();
        Self {
            files,
            additions,
            deletions,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn files_changed(&self) -> usize {
        self.files.len()
    }
}

#[derive(Debug, Clone)]
pub struct DiffFile {
    pub old_path: Option<String>,
    pub new_path: String,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub changes: u32,
    pub hunks: Vec<DiffHunk>,
    pub binary: bool,
    pub truncated: bool,
}

impl DiffFile {
    pub fn display_path(&self) -> &str {
        &self.new_path
    }

    pub fn line_count(&self) -> usize {
        self.hunks.iter().map(|hunk| hunk.lines.len()).sum()
    }

    pub fn flattened_lines(&self) -> Vec<&DiffLine> {
        self.hunks
            .iter()
            .flat_map(|hunk| hunk.lines.iter())
            .collect()
    }

    /// Rows used by the side-by-side renderer. Removed and added lines are paired
    /// inside each hunk; context lines appear on both sides.
    pub fn side_by_side_hunks(&self) -> Vec<DiffFileHunkView<'_>> {
        let mut views = Vec::new();
        let mut flat_index = 0usize;
        for hunk in &self.hunks {
            let mut rows = Vec::new();
            let mut index = 0;
            while index < hunk.lines.len() {
                let line = &hunk.lines[index];
                match line.kind {
                    DiffLineKind::Context => {
                        rows.push((Some((flat_index, line)), Some((flat_index, line))));
                        flat_index += 1;
                        index += 1;
                    }
                    DiffLineKind::Removed | DiffLineKind::Added => {
                        let mut removed = Vec::new();
                        let mut added = Vec::new();
                        while index < hunk.lines.len() {
                            match hunk.lines[index].kind {
                                DiffLineKind::Removed => {
                                    removed.push((flat_index, &hunk.lines[index]));
                                    flat_index += 1;
                                    index += 1;
                                }
                                DiffLineKind::Added => {
                                    added.push((flat_index, &hunk.lines[index]));
                                    flat_index += 1;
                                    index += 1;
                                }
                                DiffLineKind::Context => break,
                            }
                        }
                        let count = removed.len().max(added.len());
                        for row in 0..count {
                            rows.push((removed.get(row).copied(), added.get(row).copied()));
                        }
                    }
                }
            }
            views.push(DiffFileHunkView { hunk, rows });
        }
        views
    }
}

pub type SideBySideRow<'a> = (Option<(usize, &'a DiffLine)>, Option<(usize, &'a DiffLine)>);

pub struct DiffFileHunkView<'a> {
    pub hunk: &'a DiffHunk,
    pub rows: Vec<SideBySideRow<'a>>,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}
