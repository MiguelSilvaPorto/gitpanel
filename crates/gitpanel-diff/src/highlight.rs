use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::model::DiffFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub color: (u8, u8, u8),
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

pub struct Highlighter {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
    theme_name: &'static str,
}

impl Highlighter {
    pub fn new(dark: bool) -> Self {
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
            theme_name: if dark {
                "base16-ocean.dark"
            } else {
                "InspiredGitHub"
            },
        }
    }

    pub fn highlight_file(&self, file: &DiffFile) -> Vec<Vec<HighlightSpan>> {
        let Some(theme) = self.theme() else {
            return file
                .flattened_lines()
                .iter()
                .map(|line| vec![plain_span(&line.content)])
                .collect();
        };
        let extension = std::path::Path::new(file.display_path())
            .extension()
            .and_then(|value| value.to_str());
        let syntax = extension
            .and_then(|extension| self.syntaxes.find_syntax_by_extension(extension))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());

        let mut result = Vec::new();
        for hunk in &file.hunks {
            let mut highlighter = HighlightLines::new(syntax, theme);
            for line in &hunk.lines {
                let text = if line.content.is_empty() {
                    "\n".to_owned()
                } else {
                    format!("{}\n", line.content)
                };
                match highlighter.highlight_line(&text, &self.syntaxes) {
                    Ok(ranges) => {
                        let spans = ranges
                            .into_iter()
                            .filter_map(|(style, value)| {
                                let end = value.len();
                                if end == 0 {
                                    return None;
                                }
                                Some(HighlightSpan {
                                    start: 0,
                                    end,
                                    color: (
                                        style.foreground.r,
                                        style.foreground.g,
                                        style.foreground.b,
                                    ),
                                    bold: style.font_style.contains(FontStyle::BOLD),
                                    italic: style.font_style.contains(FontStyle::ITALIC),
                                    underline: style.font_style.contains(FontStyle::UNDERLINE),
                                })
                            })
                            .collect::<Vec<_>>();
                        result.push(normalize_spans(&line.content, spans));
                    }
                    Err(_) => result.push(vec![plain_span(&line.content)]),
                }
            }
        }
        result
    }

    fn theme(&self) -> Option<&Theme> {
        self.themes.themes.get(self.theme_name)
    }
}

fn normalize_spans(content: &str, spans: Vec<HighlightSpan>) -> Vec<HighlightSpan> {
    let mut result = Vec::new();
    let mut cursor = 0;
    for mut span in spans {
        if cursor >= content.len() {
            break;
        }
        let end = cursor.saturating_add(span.end).min(content.len());
        if end <= cursor {
            continue;
        }
        span.start = cursor;
        span.end = end;
        result.push(span);
        cursor = end;
    }
    if result.is_empty() {
        return vec![plain_span(content)];
    }
    if cursor < content.len() {
        result.push(plain_span_from(content, cursor));
    }
    result
}

fn plain_span(content: &str) -> HighlightSpan {
    plain_span_from(content, 0)
}

fn plain_span_from(content: &str, start: usize) -> HighlightSpan {
    HighlightSpan {
        start,
        end: content.len(),
        color: (200, 200, 200),
        bold: false,
        italic: false,
        underline: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitpanel_core::FilePatch;

    #[test]
    fn highlights_rust_lines() {
        let file = crate::parser::parse_file_patch(&FilePatch {
            filename: "src/main.rs".to_owned(),
            previous_filename: None,
            status: "modified".to_owned(),
            additions: 1,
            deletions: 0,
            changes: 1,
            patch: Some("@@ -1 +1,2 @@\n fn main() {\n+    let value = 1;\n".to_owned()),
        });
        let highlighter = Highlighter::new(true);
        let lines = highlighter.highlight_file(&file);
        assert_eq!(lines.len(), 2);
        assert!(!lines[1].is_empty());
    }

    #[test]
    fn uses_plain_text_for_unknown_extension() {
        let file = crate::parser::parse_file_patch(&FilePatch {
            filename: "README.unknown".to_owned(),
            previous_filename: None,
            status: "modified".to_owned(),
            additions: 1,
            deletions: 0,
            changes: 1,
            patch: Some("@@ -1 +1,2 @@\n hello\n+world\n".to_owned()),
        });
        let highlighter = Highlighter::new(false);
        let lines = highlighter.highlight_file(&file);
        assert_eq!(lines.len(), 2);
    }
}
