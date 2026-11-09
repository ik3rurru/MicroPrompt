//! A script to read from (MicroPrompt): a `.txt` or `.md` file the
//! Teleprompter pane shows in large type. Markdown is drawn formatted by
//! rune-kit's renderer; a `.txt` reads as written, one paragraph per block.

use std::path::Path;

/// Scripts are text; a bigger file is not one.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Script {
    /// The file's name, shown above the text.
    pub(crate) name: String,
    /// The file's text, BOM removed, with `\n` line ends.
    pub(crate) text: String,
    /// False for a `.txt`.
    pub(crate) markdown: bool,
}

impl Script {
    /// Reads `path`; Markdown unless it ends in `.txt`.
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size > MAX_BYTES {
            return Err(format!("{} MB is too large for a script", size >> 20));
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        Ok(Self {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            text: String::from_utf8_lossy(&bytes)
                .trim_start_matches('\u{feff}')
                .replace("\r\n", "\n"),
            markdown: !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("txt")),
        })
    }

    /// A Markdown script's top-level blocks (a heading, a paragraph, a whole
    /// list, a table…) as their source, so the pane spaces them for reading
    /// at a distance: rune-kit's own margins are sized for small text.
    pub(crate) fn blocks(&self) -> Vec<&str> {
        blocks(&self.text)
    }

    /// A `.txt`'s paragraphs: blank lines end one, its lines flow together.
    pub(crate) fn paragraphs(&self) -> impl Iterator<Item = String> + '_ {
        self.text
            .split("\n\n")
            .map(|block| block.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|paragraph| !paragraph.is_empty())
    }
}

/// Markdown `text`'s top-level blocks, each a slice of it
/// ([`Script::blocks`]).
pub(crate) fn blocks(text: &str) -> Vec<&str> {
    use pulldown_cmark::{Event, Options, Parser};
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut depth = 0usize;
    let mut blocks = Vec::new();
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    blocks.push(text[range].trim_end());
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            // A rule or raw HTML is a block of its own with no Start.
            _ if depth == 0 => blocks.push(text[range].trim_end()),
            _ => {}
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_flows_into_paragraphs() {
        let script = Script {
            text: "# 1 * 2\nsigue\n\n\n- dos\n".into(),
            ..Script::default()
        };
        assert_eq!(
            script.paragraphs().collect::<Vec<_>>(),
            ["# 1 * 2 sigue", "- dos"]
        );
    }

    #[test]
    fn markdown_splits_into_top_level_blocks() {
        let script = Script {
            text: "# Hola\nTexto\nsigue\n\n- uno\n  - dos\n- tres\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n".into(),
            markdown: true,
            ..Script::default()
        };
        assert_eq!(
            script.blocks(),
            [
                "# Hola",
                "Texto\nsigue",
                "- uno\n  - dos\n- tres",
                "---",
                "| a | b |\n|---|---|\n| 1 | 2 |",
            ]
        );
    }
}
