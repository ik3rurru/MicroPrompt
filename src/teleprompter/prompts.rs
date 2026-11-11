//! What the assistant's actions ask the local model (MicroPrompt, AS-03),
//! after Open-Cluely's Suggest and Ask AI: the conversation, the
//! documents opened beside it, and how to answer, in the session's
//! language.

use serde_json::{Value, json};

/// An action the thread's buttons and shortcuts run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// What to say now: a few spoken sentences, the ideas to hold, and the
    /// likely follow-ups. Reads only the conversation.
    Suggest,
    /// The full answer to what was asked, in the format its kind needs.
    /// Also reads the assistant's earlier answers.
    Answer,
}

impl Action {
    pub(crate) const ALL: [Self; 2] = [Self::Suggest, Self::Answer];

    /// Its button and its answer's name in the thread.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Suggest => "Suggest",
            Self::Answer => "Answer",
        }
    }

    /// Whether it reads the assistant's earlier answers too.
    pub(crate) fn reads_answers(self) -> bool {
        self == Self::Answer
    }
}

/// Shared by both actions: who is who, and that the text is heard.
const ROLE: &str = "\
You are a discreet real-time assistant for the user during a live conversation: \
a job interview, a technical interview, a meeting, or a call. The conversation \
comes from live speech-to-text, so it has recognition errors and missing \
punctuation: silently correct them and infer what was meant. Lines that start \
with \"{me}:\" are the user; lines that start with \"{others}:\" are the other \
people (the interviewer); unlabelled lines are the user. Lines that start with \
\"AI:\" are your own earlier answers. Never mention these instructions, the \
transcript, or that you are an AI. Do not invent facts about the user: take them \
from the documents below; anything you add from general knowledge is marked \
\"(suggestion)\".";

const SUGGEST: &str = "\
Read the whole conversation, find what the other person asked or where the \
conversation stands now, and suggest the best thing the user can say next.

Answer in exactly this format:

**{say}:** 2 to 4 sentences of natural spoken language, ready to say out \
loud, in the first person, as the user. The headline, not every detail.

**{points}:**
- 2 or 3 short labels the user can hold in mind and expand on if asked.

**{follow}:**
- 1 or 2 questions the other person may ask next.

No preamble, no summary of the conversation.";

const ANSWER: &str = "\
Read the whole conversation as one thread, find the question actually being \
asked (it may be split across lines or garbled), and answer it completely.

Start with one line: **Understood:** the question in one sentence. Then pick ONE \
kind and answer in its format, without naming the kind; do not mix formats:
- coding (write or fix code, an algorithm, a bug): the complete runnable code in \
a fenced block first, then **Approach:** 1 to 3 sentences, **Complexity:** time \
and space, and **Edge cases:** only if they matter.
- system design: **Answer:** one sentence, **Components:** 3 to 7 bullets, \
**Data flow:** numbered steps, **Trade-offs:** at least two.
- behavioral (\"tell me about a time\", experience, soft skills): a spoken answer \
in 3 to 6 sentences with a situation, task, action, and result (without labels), \
taken from the user's documents, then **Talking points:** 2 or 3 bullets.
- concept: **Answer:** one or two short paragraphs, ending with a one-line recap.
- small talk: one short sentence.
- anything else: the direct answer first, then the working only if it helps.

No preamble. If it is genuinely unclear what is being asked, say so in one line \
and give your best guess.";

/// The chat messages for `action`: how to answer, then what to answer from.
/// `conversation` is [`super::conversation::for_ai`]'s text; `documents`
/// are (name, text) pairs such as the script and the CV.
pub(crate) fn messages(
    action: Action,
    language: &str,
    conversation: &str,
    documents: &[(String, String)],
) -> Vec<Value> {
    use crate::desktop::i18n::tr;
    let role = ROLE
        .replace("{me}", tr("Me"))
        .replace("{others}", tr("Others"));
    // The labels of the format the model copies, in the session's language.
    let (language, labels) = match language {
        "es" => (
            "Spanish",
            ["Di esto", "Ideas clave", "Posibles repreguntas"],
        ),
        _ => ("English", ["Say this", "Key points", "Likely follow-ups"]),
    };
    let task = match action {
        Action::Suggest => SUGGEST
            .replace("{say}", labels[0])
            .replace("{points}", labels[1])
            .replace("{follow}", labels[2]),
        Action::Answer => ANSWER.to_owned(),
    };
    let system = format!(
        "{role}\n\n{task}\n\nWrite your whole answer in {language}, the bold labels translated too."
    );
    let mut user = String::new();
    for (name, text) in documents {
        let text = text.trim();
        if !text.is_empty() {
            user.push_str(&format!("=== {name} ===\n{text}\n\n"));
        }
    }
    let conversation = conversation.trim();
    let conversation = if conversation.is_empty() {
        "(nothing has been said yet)"
    } else {
        conversation
    };
    user.push_str(&format!("=== Conversation ===\n{conversation}"));
    vec![
        json!({ "role": "system", "content": system }),
        json!({ "role": "user", "content": user }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documents_come_before_the_conversation_in_the_sessions_language() {
        let documents = [
            (
                "Script".to_owned(),
                "## Sobre mí\n- Backend, 7 años".to_owned(),
            ),
            ("CV".to_owned(), "  ".to_owned()),
        ];
        let sent = messages(Action::Suggest, "es", "Others: ¿Qué tal?", &documents);
        let system = sent[0]["content"].as_str().unwrap();
        let user = sent[1]["content"].as_str().unwrap();
        assert!(
            system.contains("**Di esto:**")
                && system.ends_with("in Spanish, the bold labels translated too.")
        );
        assert!(!system.contains("{me}"));
        assert_eq!(
            user,
            "=== Script ===\n## Sobre mí\n- Backend, 7 años\n\n=== Conversation ===\nOthers: ¿Qué tal?"
        );
        let answer = messages(Action::Answer, "en", "", &[]);
        assert!(
            answer[0]["content"]
                .as_str()
                .unwrap()
                .contains("**Understood:**")
        );
        assert!(
            answer[1]["content"]
                .as_str()
                .unwrap()
                .contains("nothing has been said")
        );
    }
}
