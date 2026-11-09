//! The Teleprompter session's language, Spanish or English, chosen in its
//! pane before starting. It holds for this run of Hex only and never touches
//! the dictation language in Settings: the session streams on the model
//! dictation already loaded with this language as its hint, so nothing
//! reloads (Nemotron 3.5 ASR takes either; Parakeet Unified English only
//! English, and a Spanish session on it is refused with why).

use std::sync::Mutex;

use crate::control::ControlReply;
use crate::transcription::engine::TranscriptionEngine;
use crate::transcription::models::{TranscriptionSelection, definition};
use crate::transcription::stream::{Engine, shared_engine_in};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Language {
    Spanish,
    English,
}

impl Language {
    /// In the pane's order.
    pub(crate) const ALL: [Self; 2] = [Self::Spanish, Self::English];

    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Spanish => "es",
            Self::English => "en",
        }
    }

    /// Its own name, as language pickers show it.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Spanish => "Español",
            Self::English => "English",
        }
    }
}

/// The pane's pick; `None` until the person picks one.
static CHOSEN: Mutex<Option<Language>> = Mutex::new(None);

/// The session's language: the pick, else English when `dictation` (its
/// language code) is English and Spanish otherwise.
pub(crate) fn chosen(dictation: &str) -> Language {
    let picked = *CHOSEN.lock().unwrap_or_else(|error| error.into_inner());
    picked.unwrap_or_else(|| default_for(dictation))
}

pub(crate) fn choose(language: Language) {
    *CHOSEN.lock().unwrap_or_else(|error| error.into_inner()) = Some(language);
}

fn default_for(dictation: &str) -> Language {
    if dictation == "en" {
        Language::English
    } else {
        Language::Spanish
    }
}

/// Why a session in `language` can't run on `model`.
pub(crate) fn unsupported(language: Language, model: &str) -> String {
    let name = match language {
        Language::Spanish => "Spanish",
        Language::English => "English",
    };
    format!("{model} doesn't transcribe {name}; Nemotron 3.5 ASR (Settings › General) does both")
}

/// The shared engine for a session in the [`chosen`] language, its
/// model's selection, and that language; or the reply for why not.
pub(crate) fn engine() -> Result<(Box<dyn Engine>, TranscriptionSelection, Language), ControlReply>
{
    let held = TranscriptionEngine::shared().and_then(|engine| engine.held_selection());
    let language = chosen(held.as_ref().map_or("", |held| held.language.as_str()));
    match shared_engine_in(Some(language.code())) {
        Ok((engine, selection)) => Ok((engine, selection, language)),
        Err(code @ "language-unsupported") => {
            let model = held.map_or("The speech model", |held| definition(held.model).name);
            Err(ControlReply::error(code, unsupported(language, model)))
        }
        Err(code) => Err(ControlReply::error(
            code,
            "the speech model isn't loaded yet",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::models::TranscriptionModelId;

    #[test]
    fn the_default_follows_english_dictation_and_is_spanish_otherwise() {
        assert_eq!(default_for("en"), Language::English);
        assert_eq!(default_for("es"), Language::Spanish);
        assert_eq!(default_for("auto"), Language::Spanish);
    }

    #[test]
    fn nemotron_streams_either_language_and_unified_english_only_english() {
        let nemotron = definition(TranscriptionModelId::Nemotron35Asr);
        let unified = definition(TranscriptionModelId::ParakeetUnifiedEnglish);
        for language in Language::ALL {
            assert!(nemotron.supports_language(language.code()));
            assert!(nemotron.runtime_language_hint(language.code()).is_some());
        }
        assert!(unified.supports_language("en"));
        assert!(!unified.supports_language("es"));
    }
}
