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

    /// A `.txt`'s paragraphs: blank lines end one, its lines flow together.
    pub(crate) fn paragraphs(&self) -> impl Iterator<Item = String> + '_ {
        self.text
            .split("\n\n")
            .map(|block| block.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|paragraph| !paragraph.is_empty())
    }
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
}
