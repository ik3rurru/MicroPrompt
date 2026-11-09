//! A script to read from (MicroPrompt): a `.txt` or `.md` file as plain
//! blocks the Teleprompter pane shows in large type. Markdown keeps only
//! what reads aloud: headings and list items lose their `#`, `-`, and `*`,
//! emphasis and code marks go, and a link reads as its text.

use std::path::Path;

/// Scripts are text; a bigger file is not one.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Script {
    /// The file's name, shown above the text.
    pub(crate) name: String,
    pub(crate) blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Block {
    Heading(String),
    Paragraph(String),
    /// A list item: its marker ("•", "3.") and text.
    Item {
        marker: String,
        text: String,
    },
}

impl Script {
    /// Reads `path`; Markdown unless it ends in `.txt`.
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size > MAX_BYTES {
            return Err(format!("{} MB is too large for a script", size >> 20));
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let plain = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
        Ok(Self {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            blocks: parse(&text, !plain),
        })
    }
}

/// Blank lines end a block; a block's lines flow together. Without
/// `markdown`, every block is a paragraph, read as written.
pub(crate) fn parse(text: &str, markdown: bool) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    // Whether the next plain line continues the last block.
    let mut open = false;
    let mut fenced = false;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let mut line = line.trim();
        if markdown {
            if line.starts_with("```") || line.starts_with("~~~") {
                fenced = !fenced;
                open = false;
                continue;
            }
            if !fenced {
                while let Some(rest) = line.strip_prefix('>') {
                    line = rest.trim_start();
                }
            }
        }
        if line.is_empty() {
            open = false;
            continue;
        }
        if markdown && !fenced {
            if is_rule(line) {
                open = false;
                continue;
            }
            if let Some(heading) = heading(line) {
                blocks.extend(
                    Some(inline(heading))
                        .filter(|h| !h.is_empty())
                        .map(Block::Heading),
                );
                open = false;
                continue;
            }
            if let Some((marker, text)) = item(line) {
                blocks.push(Block::Item {
                    marker,
                    text: inline(text),
                });
                open = true;
                continue;
            }
        }
        let line = if markdown && !fenced {
            inline(line)
        } else {
            line.to_owned()
        };
        match blocks.last_mut() {
            Some(Block::Paragraph(text) | Block::Item { text, .. }) if open => {
                text.push(' ');
                text.push_str(&line);
            }
            _ => blocks.push(Block::Paragraph(line)),
        }
        open = true;
    }
    blocks
}

/// `---`, `***`, `___`, and setext `===` underlines.
fn is_rule(line: &str) -> bool {
    let marks: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    marks.len() >= 3
        && ['-', '*', '_', '=']
            .iter()
            .any(|mark| marks.chars().all(|c| c == *mark))
}

fn heading(line: &str) -> Option<&str> {
    let level = line.chars().take_while(|c| *c == '#').count();
    let rest = line.get(level..)?;
    ((1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' ')))
        .then(|| rest.trim().trim_end_matches('#').trim_end())
}

fn item(line: &str) -> Option<(String, &str)> {
    let (marker, rest) = if let Some(rest) = ["- ", "* ", "+ "]
        .iter()
        .find_map(|bullet| line.strip_prefix(bullet))
    {
        ("•".to_owned(), rest)
    } else {
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let (number, rest) = line.split_at(digits);
        let rest = rest
            .strip_prefix(". ")
            .or_else(|| rest.strip_prefix(") "))?;
        (format!("{number}."), rest)
    };
    let rest = rest.trim_start();
    let rest = ["[ ] ", "[x] ", "[X] "]
        .iter()
        .find_map(|task| rest.strip_prefix(task))
        .unwrap_or(rest);
    Some((marker, rest))
}

/// Links and images read as their text; emphasis, strike, and code marks
/// go.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let link = rest[open..].find("](").and_then(|close| {
            let url = open + close + 2;
            rest[url..]
                .find(')')
                .map(|end| (open + close, url + end + 1))
        });
        let Some((close, end)) = link else { break };
        out.push_str(rest[..open].strip_suffix('!').unwrap_or(&rest[..open]));
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out.replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace(['*', '`'], "")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_reads_without_its_marks() {
        let text = "\u{feff}# Bienvenida #\n\nHola a **todos**, esto es\nun *guion*.\n\n\
                    ---\n> Una cita\n\n- Primero\n  sigue aquí\n* [x] Segundo con `código`\n\
                    12) Doce\n\nVer [la web](https://x.y) y ![logo](a.png).\n\n```\n# no es título\n```\n";
        assert_eq!(
            parse(text, true),
            vec![
                Block::Heading("Bienvenida".into()),
                Block::Paragraph("Hola a todos, esto es un guion.".into()),
                Block::Paragraph("Una cita".into()),
                Block::Item {
                    marker: "•".into(),
                    text: "Primero sigue aquí".into()
                },
                Block::Item {
                    marker: "•".into(),
                    text: "Segundo con código".into()
                },
                Block::Item {
                    marker: "12.".into(),
                    text: "Doce".into()
                },
                Block::Paragraph("Ver la web y logo.".into()),
                Block::Paragraph("# no es título".into()),
            ]
        );
    }

    #[test]
    fn plain_text_keeps_its_marks() {
        assert_eq!(
            parse("# 1 * 2\nsigue\n\n- dos", false),
            vec![
                Block::Paragraph("# 1 * 2 sigue".into()),
                Block::Paragraph("- dos".into()),
            ]
        );
        assert_eq!(heading("#hashtag"), None);
    }
}
