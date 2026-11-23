#![forbid(unsafe_code)]

pub mod highlight;
pub mod model;
pub mod parser;

pub use highlight::{HighlightSpan, Highlighter};
pub use model::{Diff, DiffFile, DiffFileHunkView, DiffHunk, DiffLine, DiffLineKind, FileStatus};
pub use parser::{parse_file_patch, parse_hunks};
