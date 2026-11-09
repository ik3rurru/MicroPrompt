//! MicroPrompt's interview preparation (Labs › Interview prep): an
//! interview script (Markdown, `tools/microprompt-interview`'s template) as
//! its `## ` sections, the questions to ask at the end as a checklist, and
//! the documents that go with it (CV, offer, company notes). Everything is
//! read from local files and stays in memory.

use std::path::Path;

use super::script::Script;

/// The documents beside the script: what the pane calls each, and the file
/// names (`.md` or `.txt`) taken from the script's folder when it opens.
pub(crate) const DOCUMENTS: [(&str, &[&str]); 3] = [
    ("CV", &["cv", "curriculum", "resume"]),
    ("Offer", &["oferta", "offer"]),
    ("Company notes", &["empresa", "company"]),
];

/// One `## ` section: its heading's text and the Markdown under it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Section {
    pub(crate) title: String,
    pub(crate) body: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Interview {
    pub(crate) sections: Vec<Section>,
    /// As [`DOCUMENTS`].
    pub(crate) documents: [Option<Script>; 3],
}

impl Interview {
    /// The script's sections, comments left out; `None` when it has none.
    /// The section of questions to ask ("Preguntas que haré") gets its list
    /// as tasks, to tick off ([`Self::tick`]).
    pub(crate) fn read(script: &Script) -> Option<Self> {
        let mut sections: Vec<Section> = Vec::new();
        for line in without_comments(&script.text).lines() {
            if let Some(title) = line.strip_prefix("## ") {
                sections.push(Section {
                    title: title.trim().to_string(),
                    body: String::new(),
                });
            } else if let Some(section) = sections.last_mut() {
                section.body.push_str(line);
                section.body.push('\n');
            }
        }
        for section in &mut sections {
            let title = section.title.to_lowercase();
            if ["que haré", "to ask", "i'll ask"]
                .iter()
                .any(|own| title.contains(own))
            {
                section.body = as_tasks(&section.body);
            }
        }
        (!sections.is_empty()).then(|| Self {
            sections,
            documents: Default::default(),
        })
    }

    /// Takes the documents found by name in `folder`, the script's.
    pub(crate) fn gather(&mut self, folder: &Path) {
        for (document, (_, names)) in self.documents.iter_mut().zip(DOCUMENTS) {
            *document = names
                .iter()
                .flat_map(|name| ["md", "txt"].map(|ext| folder.join(format!("{name}.{ext}"))))
                .find_map(|path| Script::open(&path).ok());
        }
    }

    /// Ticks or clears the task whose `[` is at `offset` of `section`'s
    /// body; a ticked question is struck through.
    pub(crate) fn tick(&mut self, section: usize, offset: usize, checked: bool) {
        let Some(body) = self.sections.get_mut(section).map(|it| &mut it.body) else {
            return;
        };
        let Some(rest) = body.get(offset..).filter(|rest| rest.starts_with('[')) else {
            return;
        };
        let end = offset + rest.find('\n').unwrap_or(rest.len());
        let Some(text) = body.get(offset + 3..end).map(str::trim) else {
            return;
        };
        let text = text.strip_prefix("~~").unwrap_or(text);
        let text = text.strip_suffix("~~").unwrap_or(text);
        let line = if checked {
            format!("[x] ~~{text}~~")
        } else {
            format!("[ ] {text}")
        };
        body.replace_range(offset..end, &line);
    }
}

/// `text` less its `<!-- … -->` comments (the template's instructions).
fn without_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        rest = rest[start..]
            .find("-->")
            .map_or("", |end| &rest[start + end + 3..]);
    }
    out + rest
}

/// `body` with each list item that says something as an unticked task.
fn as_tasks(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines() {
        let item = line.trim_start();
        let indent = &line[..line.len() - item.len()];
        match item.strip_prefix("- ").or(item.strip_prefix("* ")) {
            Some(text) if !text.trim().is_empty() && !text.starts_with('[') => {
                out.push_str(&format!("{indent}- [ ] {}", text.trim()));
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_reads_by_sections_and_its_questions_tick_off() {
        let script = Script {
            text: "# Guion\n\n<!-- cómo\n## rellenarlo -->\n\n## Sobre mí\n\n- Backend 7 años\n\n\
                   ## Preguntas que haré\n\n- ¿Cómo medís el éxito?\n-\n- ¿Y las guardias?\n"
                .into(),
            markdown: true,
            ..Script::default()
        };
        let mut interview = Interview::read(&script).unwrap();
        let titles = interview.sections.iter().map(|it| it.title.as_str());
        assert_eq!(
            titles.collect::<Vec<_>>(),
            ["Sobre mí", "Preguntas que haré"]
        );
        assert_eq!(interview.sections[0].body, "\n- Backend 7 años\n\n");
        let questions = "\n- [ ] ¿Cómo medís el éxito?\n-\n- [ ] ¿Y las guardias?\n";
        assert_eq!(interview.sections[1].body, questions);

        let second = questions.rfind('[').unwrap();
        interview.tick(1, second, true);
        assert_eq!(
            interview.sections[1].body,
            "\n- [ ] ¿Cómo medís el éxito?\n-\n- [x] ~~¿Y las guardias?~~\n"
        );
        interview.tick(1, second, false);
        assert_eq!(interview.sections[1].body, questions);
        // Not a task's marker: nothing changes.
        interview.tick(1, 0, true);
        interview.tick(7, 0, true);
        assert_eq!(interview.sections[1].body, questions);

        // The documents beside the script are taken by name.
        let folder = std::env::temp_dir().join(format!("hex-interview-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("cv.md"), "# Alex").unwrap();
        std::fs::write(folder.join("oferta.txt"), "Backend").unwrap();
        interview.gather(&folder);
        std::fs::remove_dir_all(&folder).unwrap();
        let names = interview
            .documents
            .each_ref()
            .map(|it| it.as_ref().map(|it| it.name.as_str()));
        assert_eq!(names, [Some("cv.md"), Some("oferta.txt"), None]);

        let plain = Script {
            text: "# Sólo un título\n\nTexto.\n".into(),
            ..Script::default()
        };
        assert_eq!(Interview::read(&plain), None);
    }
}
