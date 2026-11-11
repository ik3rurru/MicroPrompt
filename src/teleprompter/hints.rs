//! The interview's automatic help (MicroPrompt, EN-03), ported from the
//! EN-01 prototype (tools/microprompt-interview/apuntador.py): when the
//! interviewer's turn becomes a question, the thread gets a hint with three
//! key points: the user's own when the question is one of the script's
//! "Preguntas previstas" (worded differently or not), else the local
//! model's. Only "Others" is listened to, so the user's own words never
//! fire one.

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

/// Words that say nothing about which question it is.
const STOPWORDS: &str = "a al algo como con cual cuales de del donde el ella en era es esa ese \
    eso esta este esto fue ha has hay la las le les lo los me mi mis muy nos nosotros nuestra \
    nuestro o os para pero por porque que quien se si sin sobre su sus te ti tu tus un una uno \
    unos y ya yo vosotros hablame cuentame dime explicame describeme comentame quieres quiero \
    querrias gustaria puedes podrias haces hace son estas seria eres vale bueno pues entonces \
    voy vas va van the and you your what how why tell about can could would are is do did have \
    for with that this";
/// Crude stemming, so "cobrar" meets "cobro".
const SUFFIXES: [&str; 20] = [
    "aciones", "acion", "amente", "mente", "ando", "iendo", "ados", "adas", "ado", "ada", "ar",
    "er", "ir", "os", "as", "es", "o", "a", "e", "s",
];

/// Lower case without accents (ñ becomes n).
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase()
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect()
}

fn words(text: &str) -> Vec<String> {
    fold(text)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The content words of `text`, stemmed and cut to five letters.
fn stems(text: &str) -> HashSet<String> {
    let stop: HashSet<&str> = STOPWORDS.split_whitespace().collect();
    words(text)
        .into_iter()
        .filter(|word| word.len() >= 3 && !stop.contains(word.as_str()))
        .map(|word| {
            let cut = SUFFIXES
                .iter()
                .find(|suffix| word.ends_with(*suffix) && word.len() - suffix.len() >= 3)
                .map_or(word.as_str(), |suffix| &word[..word.len() - suffix.len()]);
            cut.chars().take(5).collect()
        })
        .collect()
}

/// One of the script's "Preguntas previstas": how it may be asked and the
/// user's points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prepared {
    pub name: String,
    phrasings: Vec<String>,
    pub points: Vec<String>,
}

/// The script's prepared questions (`## Preguntas previstas`, each a `###`
/// with an optional `También:` line of `/`-separated phrasings and `-`
/// points), and "Preguntas que haré" as the answer to the interviewer's
/// closing "¿tienes alguna pregunta?". English titles work too ("Expected
/// questions", "Also:", "Questions to ask").
pub(crate) fn prepared(script: &str) -> Vec<Prepared> {
    let script = regex::Regex::new(r"(?s)<!--.*?-->")
        .expect("a valid pattern")
        .replace_all(script, "");
    let mut prepared: Vec<Prepared> = Vec::new();
    let mut ask = Vec::new();
    let mut section = String::new();
    for line in script.lines().map(str::trim) {
        if let Some(title) = line.strip_prefix("## ") {
            section = fold(title);
            continue;
        }
        let point = line
            .strip_prefix("- ")
            .map(str::trim)
            .filter(|it| !it.is_empty());
        if section.starts_with("preguntas previstas") || section.contains("expected questions") {
            if let Some(name) = line.strip_prefix("### ") {
                let name = name.trim().to_owned();
                prepared.push(Prepared {
                    phrasings: vec![name.clone()],
                    name,
                    points: Vec::new(),
                });
            } else if let Some(last) = prepared.last_mut() {
                let folded = fold(line);
                if let Some(also) = ["tambien:", "also:"]
                    .iter()
                    .find_map(|label| folded.strip_prefix(label).map(|_| label.len()))
                {
                    let also = line
                        .char_indices()
                        .nth(also)
                        .map_or("", |(at, _)| &line[at..]);
                    last.phrasings.extend(
                        also.split('/')
                            .map(str::trim)
                            .filter(|it| !it.is_empty())
                            .map(str::to_owned),
                    );
                } else if let Some(point) = point {
                    last.points.push(point.to_owned());
                }
            }
        } else if (section.starts_with("preguntas que hare") || section.contains("to ask"))
            && let Some(point) = point
        {
            ask.push(point.to_owned());
        }
    }
    if !ask.is_empty() {
        ask.truncate(3);
        prepared.push(Prepared {
            name: "Preguntas que haré".into(),
            // Each needs "pregunta": "¿Tienes alguna experiencia…?" is no closing.
            phrasings: vec![
                "alguna pregunta para nosotros".into(),
                "alguna pregunta que quieras hacernos".into(),
                "any questions for us".into(),
            ],
            points: ask,
        });
    }
    prepared
}

/// The prepared question whose phrasing's content words best appear in
/// `question`: at least two of them and 60 % of the phrasing's.
pub(crate) fn matching<'a>(question: &str, prepared: &'a [Prepared]) -> Option<&'a Prepared> {
    let heard = stems(question);
    let mut best = None;
    let mut best_score = 0.0;
    for entry in prepared {
        for phrasing in &entry.phrasings {
            let wanted = stems(phrasing);
            let common = wanted.intersection(&heard).count();
            let score = common as f64 / wanted.len().max(1) as f64;
            if common >= 2 && score > best_score {
                best = Some(entry);
                best_score = score;
            }
        }
    }
    best.filter(|_| best_score >= 0.6)
}

/// How a clause opens like a request ("Cuéntame…"). Nemotron often drops
/// the question marks, so these matter.
const REQUESTS: &str = "cuentame cuentanos hablame hablanos explicame describeme dime comentame \
    imagina ponme gustaria queria quisiera tell describe explain walk";
/// Second-person verbs: a question with some substance ("Tienes alguna
/// pregunta…", not "Tienes razón").
const YOU_VERBS: &str = "haces cuentas dices explicas describes puedes podrias sabes tienes has \
    conoces quieres crees piensas opinas ves consideras prefieres gestionas manejas valoras \
    dirias harias can could would do did have are";
/// Interrogatives, only in a sentence's last clause and with substance
/// ("Como te decía, somos seis" is no question). "cuando" is left out:
/// without "?" it is nearly always "when".
const ASKING: &str = "que como cual cuales donde cuanto cuanta cuantos cuantas quien quienes \
    what how why which where who";
const NOT_CONDITIONAL: &str = "varias diarias necesarias historias memorias primarias";
const FILLER: &str = "y e bueno vale pues entonces ahora me nos te a ver so and well";

/// A question mark, or a clause that opens like a question or a request
/// ("Cuéntame…", "Para empezar, me haces…", "Cómo diseñarías…").
pub(crate) fn is_question(text: &str) -> bool {
    if text.contains('?') || text.contains('¿') {
        return true;
    }
    let set = |list: &'static str| list.split_whitespace().collect::<HashSet<_>>();
    let (requests, you, asking) = (set(REQUESTS), set(YOU_VERBS), set(ASKING));
    let (not_conditional, filler) = (set(NOT_CONDITIONAL), set(FILLER));
    let folded = fold(text);
    for sentence in folded.split(['.', '!', ':', ';']) {
        let clauses: Vec<&str> = sentence.split(',').collect();
        for (index, clause) in clauses.iter().enumerate() {
            let words: Vec<&str> = clause
                .split(|c: char| !c.is_ascii_alphabetic())
                .filter(|word| !word.is_empty())
                .skip_while(|word| filler.contains(word))
                .collect();
            let Some(first) = words.first() else {
                continue;
            };
            let last = index == clauses.len() - 1;
            // Conditionals: diseñarías, harías, podrías, sabrías; not historias.
            let conditional = first.len() > 4
                && first.ends_with("rias")
                && "aeidb".contains(&first[first.len() - 5..first.len() - 4])
                && !not_conditional.contains(first);
            if requests.contains(first)
                || ((you.contains(first) || conditional) && words.len() >= 3)
            {
                return true;
            }
            if last && words.len() >= 4 && (asking.contains(first) || words[..2] == ["por", "que"])
            {
                return true;
            }
        }
    }
    false
}

/// What a hint says, as Markdown: where its points come from, the
/// question, and three points.
pub(crate) fn from_script(question: &str, entry: &Prepared) -> String {
    use crate::desktop::i18n::tr;
    let mut text = format!("*{}: {}*\n\n", tr("From your script"), entry.name);
    if fold(&entry.name) != fold(question) {
        text.push_str(&format!("> {}\n\n", question.trim()));
    }
    for point in entry.points.iter().take(3) {
        text.push_str(&format!("- {point}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = "\
# Guion

<!-- ## Preguntas previstas en un comentario no cuentan -->

## Sobre mí

- 7 años backend

## Preguntas previstas

### Háblame de ti
También: preséntate / resume tu trayectoria / cuéntame tu experiencia / tu perfil

- 7 años backend, Python y Go
- Logística y pagos
- Lidero 4 personas

### ¿Por qué quieres trabajar en Rutalia?
También: qué te atrae de la empresa / por qué nosotros / qué te ha llamado la atención de la compañía

- Vuelvo a logística
- Migración a eventos
- Impacto visible

### ¿Cuáles son tus expectativas salariales?
También: cuánto quieres cobrar / qué sueldo buscas / rango salarial / cuánto ganas

- Rango 58–64 k brutos
- Flexible si hay remoto
- Primero entender el puesto

## Preguntas que haré

- ¿Cómo medís el éxito del equipo?
- ¿Qué parte de la migración queda?
- ¿Cómo son las guardias?
";

    #[test]
    fn the_scripts_prepared_questions_and_closing_ones() {
        let prepared = prepared(SCRIPT);
        let names: Vec<_> = prepared.iter().map(|it| it.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Háblame de ti",
                "¿Por qué quieres trabajar en Rutalia?",
                "¿Cuáles son tus expectativas salariales?",
                "Preguntas que haré",
            ]
        );
        assert!(prepared.iter().all(|it| it.points.len() == 3));
        assert_eq!(prepared[0].phrasings.len(), 5);
    }

    #[test]
    fn reworded_questions_find_their_entry_and_others_none() {
        let prepared = prepared(SCRIPT);
        let found = |question: &str| matching(question, &prepared).map(|it| it.name.as_str());
        assert_eq!(
            found("Para empezar, resume tu trayectoria profesional"),
            Some("Háblame de ti")
        );
        assert_eq!(
            found("cuanto te gustaria cobrar"),
            Some("¿Cuáles son tus expectativas salariales?")
        );
        assert_eq!(
            found("Y qué te atrae de nuestra empresa"),
            Some("¿Por qué quieres trabajar en Rutalia?")
        );
        assert_eq!(
            found("¿Tienes alguna pregunta para nosotros?"),
            Some("Preguntas que haré")
        );
        for unexpected in [
            "¿Tienes alguna experiencia con Kafka?",
            "Tienes alguna duda técnica sobre Kubernetes",
            "Cómo invertirías una lista enlazada",
        ] {
            assert_eq!(found(unexpected), None, "{unexpected}");
        }
    }

    #[test]
    fn questions_and_requests_without_question_marks() {
        for text in [
            "¿Me haces un resumen de tu trayectoria?",
            "Cuéntame una situación difícil.",
            "Y qué harías si te cambian las prioridades",
            "Háblame de un proyecto.",
            "Vale. Por qué quieres cambiar de empresa",
            "Para empezar, me haces un pequeño resumen de tu trayectoria profesional.",
            "Como diseñarías un sistema para que los repartidores reciban los cambios de ruta.",
            "Tienes alguna pregunta para nosotros.",
            "Me gustaría que me contaras cómo trabajas en equipo.",
            "Quería preguntarte por tu último proyecto.",
            "Tell me about a time you failed.",
        ] {
            assert!(is_question(text), "{text}");
        }
    }

    #[test]
    fn statements_are_not_questions() {
        for text in [
            "Vale, perfecto, muy interesante.",
            "Te explico cómo va a ser la entrevista: primero hablamos de ti.",
            "Genial. Te cuento un poco del puesto: el equipo son seis personas.",
            "Hola, buenos días, gracias por venir.",
            "Bueno.",
            "Pues por nuestra parte eso es todo.",
            "Como te decía, somos seis personas.",
            "Cuando quieras empezamos.",
            "Tienes razón.",
            "Que vaya bien.",
            "Historias como esa nos encantan.",
        ] {
            assert!(!is_question(text), "{text}");
        }
    }

    #[test]
    fn a_hint_from_the_script_quotes_a_reworded_question() {
        let prepared = prepared(SCRIPT);
        let hint = from_script("cuanto te gustaria cobrar", &prepared[2]);
        assert!(hint.contains("¿Cuáles son tus expectativas salariales?*"));
        assert!(hint.contains("> cuanto te gustaria cobrar"));
        assert!(hint.ends_with("- Primero entender el puesto\n"));
    }
}
