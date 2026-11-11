//! MicroPrompt's local model (Labs › Assistant): Ollama on this computer,
//! over loopback HTTP, so nothing leaves it. Ollama's cloud models are left
//! out. [`chat`] is the client; [`ask`] runs the thread's actions (AS-03)
//! into the session's thread; [`view`] and [`check`] are what the
//! Teleprompter pane shows of Ollama.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use super::prompts::Action;
use crate::app_settings::AppSettings;

/// Ollama's own port, on this computer only.
const ADDRESS: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 11434);
/// EN-01's pick: the shortest, clearest points that keep to the script.
pub(crate) const DEFAULT_MODEL: &str = "gemma4:12b";
/// How long Ollama keeps the model loaded after a request: a call's quiet
/// stretches, so an answer never waits for the model to load again.
const KEEP_ALIVE: &str = "30m";
/// Ollama answers on its port at once when it runs.
const CONNECT: Duration = Duration::from_millis(500);
/// Between two pieces of an answer; a cold model takes seconds to load.
const READ: Duration = Duration::from_secs(120);

/// A model installed in Ollama.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Model {
    pub name: String,
    /// Thinks before answering unless told not to (`think: false`), which
    /// only slows a live answer.
    pub thinks: bool,
}

/// The installed models that run here: no cloud models, no embedding-only
/// ones.
pub(crate) fn models() -> io::Result<Vec<Model>> {
    let mut body = String::new();
    request("GET", "/api/tags", None)?.read_to_string(&mut body)?;
    Ok(local_models(&serde_json::from_str(&body)?))
}

fn local_models(tags: &Value) -> Vec<Model> {
    let has = |model: &Value, capability: &str| {
        model["capabilities"]
            .as_array()
            .is_some_and(|all| all.iter().any(|it| it == capability))
    };
    tags["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| model.get("remote_host").is_none() && has(model, "completion"))
        .filter_map(|model| {
            Some(Model {
                name: model["name"].as_str()?.to_owned(),
                thinks: has(model, "thinking"),
            })
        })
        .collect()
}

/// Sends `messages` (`{"role", "content"}`, oldest first) to `model` and
/// hands each piece of the answer to `piece` as it arrives; returns the
/// whole answer.
pub(crate) fn chat(
    model: &Model,
    messages: Vec<Value>,
    mut piece: impl FnMut(&str),
) -> io::Result<String> {
    let mut body = json!({
        "model": model.name,
        "messages": messages,
        "stream": true,
        "keep_alive": KEEP_ALIVE,
    });
    if model.thinks {
        body["think"] = json!(false);
    }
    let reader = request("POST", "/api/chat", Some(&serde_json::to_vec(&body)?))?;
    let mut answer = String::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(&line)?;
        if let Some(error) = event["error"].as_str() {
            return Err(io::Error::other(error.to_owned()));
        }
        if let Some(text) = event["message"]["content"]
            .as_str()
            .filter(|it| !it.is_empty())
        {
            answer.push_str(text);
            piece(text);
        }
        if event["done"] == true {
            break;
        }
    }
    Ok(answer)
}

/// One request, its body left to read. HTTP/1.0, so Ollama (Go's server)
/// streams the body without chunked framing and closes at its end.
fn request(method: &str, path: &str, body: Option<&[u8]>) -> io::Result<BufReader<TcpStream>> {
    #[cfg(test)]
    let address = tests::ADDRESS_OVERRIDE
        .with(std::cell::Cell::get)
        .unwrap_or(ADDRESS);
    #[cfg(not(test))]
    let address = ADDRESS;
    let mut stream = TcpStream::connect_timeout(&address, CONNECT)?;
    stream.set_read_timeout(Some(READ))?;
    stream.set_write_timeout(Some(READ))?;
    let body = body.unwrap_or_default();
    write!(
        stream,
        "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    let mut reader = BufReader::new(stream);
    let mut status = String::new();
    reader.read_line(&mut status)?;
    let ok = status.split_whitespace().nth(1) == Some("200");
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
    }
    if ok {
        return Ok(reader);
    }
    let mut error = String::new();
    reader.take(64 * 1024).read_to_string(&mut error)?;
    let error = serde_json::from_str::<Value>(&error)
        .ok()
        .and_then(|it| it["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| status.trim().to_owned());
    Err(io::Error::other(error))
}

/// Whether the error means nothing listens on Ollama's port.
fn not_running(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused | io::ErrorKind::TimedOut
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Status {
    /// Not looked yet: the pane asks once it shows.
    #[default]
    Unchecked,
    Checking,
    Running,
    NotRunning,
}

/// What the Teleprompter pane shows of the assistant.
#[derive(Clone, Debug, Default)]
pub(crate) struct View {
    pub generation: u64,
    pub status: Status,
    pub models: Vec<Model>,
    pub error: Option<String>,
}

impl View {
    /// `chosen` (`labs.assistant_model`, else [`DEFAULT_MODEL`]) when it is
    /// installed, else the first installed model.
    pub(crate) fn model(&self, chosen: Option<&str>) -> Option<&Model> {
        let chosen = chosen.unwrap_or(DEFAULT_MODEL);
        self.models
            .iter()
            .find(|model| model.name == chosen)
            .or(self.models.first())
    }
}

static VIEW: Mutex<View> = Mutex::new(View {
    generation: 0,
    status: Status::Unchecked,
    models: Vec::new(),
    error: None,
});

pub(crate) fn view() -> View {
    lock().clone()
}

pub(crate) fn generation() -> u64 {
    lock().generation
}

fn lock() -> std::sync::MutexGuard<'static, View> {
    VIEW.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn update(change: impl FnOnce(&mut View)) {
    {
        let mut view = lock();
        change(&mut view);
        view.generation += 1;
    }
    super::wake();
}

/// Looks for Ollama and its models on a thread of its own.
pub(crate) fn check() {
    if lock().status == Status::Checking {
        return;
    }
    update(|view| view.status = Status::Checking);
    std::thread::spawn(|| {
        let found = models();
        update(|view| match found {
            Ok(models) => {
                view.status = Status::Running;
                view.models = models;
                view.error = None;
            }
            Err(error) if not_running(&error) => {
                view.status = Status::NotRunning;
                view.models.clear();
            }
            Err(error) => {
                view.status = Status::NotRunning;
                view.error = Some(error.to_string());
            }
        });
    });
}

/// The documents opened beside the session (the script, the CV, the
/// offer, the company notes), as (name, text), which every action reads.
static DOCUMENTS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// What the window has open now; it calls this whenever that changes.
pub(crate) fn set_documents(documents: Vec<(String, String)>) {
    *DOCUMENTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = documents;
}

/// The model the saved settings choose, looking for the installed ones
/// first when the pane hasn't.
fn chosen(settings: &AppSettings) -> io::Result<Model> {
    if lock().models.is_empty() {
        let found = models()?;
        update(|view| {
            view.status = Status::Running;
            view.models = found;
        });
    }
    let chosen = settings.labs.assistant_model.as_deref();
    lock()
        .model(chosen)
        .cloned()
        .ok_or_else(|| io::Error::other("Ollama has no local models: ollama pull gemma4:12b"))
}

/// Loads the chosen model while a session starts, so the first answer
/// doesn't wait for it; quiet when Ollama isn't running.
pub(crate) fn warm() {
    std::thread::spawn(|| {
        let settings = AppSettings::read().unwrap_or_default();
        let Ok(model) = chosen(&settings) else {
            return;
        };
        let body = json!({ "model": model.name, "messages": [], "keep_alive": KEEP_ALIVE });
        let loaded = (|| -> io::Result<()> {
            let body = serde_json::to_vec(&body)?;
            let mut reply = String::new();
            request("POST", "/api/chat", Some(&body))?.read_to_string(&mut reply)?;
            Ok(())
        })();
        match loaded {
            Ok(()) => tracing::info!(model = model.name, "assistant model loaded"),
            Err(error) => tracing::warn!(%error, "assistant model didn't load"),
        }
    });
}

/// Runs `action` on what the thread lets the assistant read, its answer
/// streaming into the thread after what has been heard. One at a time: a
/// press while one answers does nothing.
pub(crate) fn ask(action: Action) {
    let Some(index) = begin(action, true) else {
        return;
    };
    std::thread::spawn(move || stream(index, action, None));
}

/// The interview's hint for `question` (EN-03): the script's own points at
/// once when it is one of its prepared questions, else, with Labs ›
/// Assistant on, the model's three, streaming. It never waits for an
/// answer in progress.
pub(crate) fn hint(question: &str) {
    let script = documents()
        .into_iter()
        .find(|(name, _)| name == "Script")
        .map(|(_, text)| text)
        .unwrap_or_default();
    let prepared = super::hints::prepared(&script);
    if let Some(entry) = super::hints::matching(question, &prepared) {
        tracing::info!(entry = entry.name, "interview hint from the script");
        let text = super::hints::from_script(question, entry);
        if let Some(index) = begin(Action::Hint, false) {
            super::update(|live| {
                if let Some(answer) = live.view.answers.get_mut(index) {
                    answer.text = text;
                    answer.done = true;
                }
            });
        }
        return;
    }
    let settings = AppSettings::read().unwrap_or_default();
    if !crate::labs::Feature::Assistant.on(&settings) {
        return;
    }
    if let Some(index) = begin(Action::Hint, false) {
        let question = question.to_owned();
        std::thread::spawn(move || stream(index, Action::Hint, Some(question)));
    }
}

fn documents() -> Vec<(String, String)> {
    DOCUMENTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Answers `action` (for a hint, its `question`) into the thread's answer
/// `index`, piece by piece.
fn stream(index: usize, action: Action, question: Option<String>) {
    {
        let started = std::time::Instant::now();
        let settings = AppSettings::read().unwrap_or_default();
        let language = super::language::chosen(&settings.transcription.language);
        let session = super::view();
        let conversation = super::conversation::for_ai(
            &session,
            super::conversation::BUDGET_CHARS,
            action.reads_answers(),
        );
        let documents = documents();
        let messages = super::prompts::messages(
            action,
            language.code(),
            &conversation,
            question.as_deref(),
            &documents,
        );
        let mut first = None;
        let result = chosen(&settings).and_then(|model| {
            chat(&model, messages, |piece| {
                first.get_or_insert_with(|| started.elapsed());
                super::update(|live| {
                    if let Some(answer) = live.view.answers.get_mut(index) {
                        answer.text.push_str(piece);
                    }
                });
            })
            .map(|_| model.name)
        });
        tracing::info!(
            action = action.label(),
            first_ms = first.map(|it| it.as_millis() as u64),
            total_ms = started.elapsed().as_millis() as u64,
            ok = result.is_ok(),
            "assistant answered"
        );
        if let Err(error) = &result
            && not_running(error)
        {
            update(|view| view.status = Status::NotRunning);
        }
        super::update(|live| {
            if let Some(answer) = live.view.answers.get_mut(index) {
                answer.done = true;
                answer.error = result.err().map(|error| {
                    if not_running(&error) {
                        crate::desktop::i18n::tr("Ollama isn't running").to_owned()
                    } else {
                        error.to_string()
                    }
                });
            }
        });
    }
}

/// Adds `action`'s answer to the thread; its index. An `exclusive` one (the
/// buttons') is refused while another of them still answers; hints never
/// wait.
fn begin(action: Action, exclusive: bool) -> Option<usize> {
    let mut index = None;
    super::update(|live| {
        let view = &mut live.view;
        let busy = view
            .answers
            .iter()
            .any(|answer| !answer.done && answer.action != Action::Hint);
        if exclusive && busy {
            return;
        }
        let open = !format!("{}{}", view.stable, view.tentative)
            .trim()
            .is_empty();
        view.answers.push(super::Answer {
            action,
            after: view.segments.len() + usize::from(open),
            text: String::new(),
            done: false,
            error: None,
            used: true,
        });
        index = Some(view.answers.len() - 1);
    });
    index
}

/// Lets the assistant read answer `index` again, or leaves it out (its
/// switch in the thread).
pub(crate) fn use_answer_in_ai(index: usize, used: bool) {
    super::update(|live| {
        if let Some(answer) = live.view.answers.get_mut(index) {
            answer.used = used;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        /// A stand-in for Ollama's port, for this test's thread.
        pub(super) static ADDRESS_OVERRIDE: std::cell::Cell<Option<SocketAddr>> =
            const { std::cell::Cell::new(None) };
    }

    #[test]
    fn only_local_models_that_answer_are_listed() {
        let tags = json!({ "models": [
            { "name": "gemma4:12b", "capabilities": ["completion", "vision", "thinking"] },
            { "name": "kimi:cloud", "remote_host": "https://ollama.com", "capabilities": ["completion"] },
            { "name": "nomic-embed-text", "capabilities": ["embedding"] },
            { "name": "llama3.2", "capabilities": ["completion"] },
        ]});
        let names: Vec<_> = local_models(&tags)
            .into_iter()
            .map(|it| (it.name, it.thinks))
            .collect();
        assert_eq!(
            names,
            [("gemma4:12b".into(), true), ("llama3.2".into(), false)]
        );
    }

    #[test]
    fn the_chosen_model_else_the_first_installed() {
        let model = |name: &str| Model {
            name: name.into(),
            thinks: false,
        };
        let view = View {
            models: vec![model("qwen3.5:9b"), model("gemma4:12b")],
            ..View::default()
        };
        assert_eq!(view.model(None).unwrap().name, "gemma4:12b");
        assert_eq!(view.model(Some("qwen3.5:9b")).unwrap().name, "qwen3.5:9b");
        assert_eq!(view.model(Some("gone")).unwrap().name, "qwen3.5:9b");
        assert!(View::default().model(None).is_none());
    }

    /// A stand-in Ollama on a free port answers one request with `reply`.
    fn serve(reply: &'static str) -> SocketAddr {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // The whole request first: closing with bytes unread resets the
            // connection on Windows before the reply is read.
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !complete(&request) {
                let read = stream.read(&mut buffer).unwrap();
                assert!(read > 0, "the request ended early");
                request.extend_from_slice(&buffer[..read]);
            }
            stream.write_all(reply.as_bytes()).unwrap();
        });
        address
    }

    /// Whether `request` holds its headers and all the body they announce.
    fn complete(request: &[u8]) -> bool {
        let text = String::from_utf8_lossy(request);
        let Some(end) = text.find("\r\n\r\n") else {
            return false;
        };
        let length = text[..end]
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|it| it.trim().parse::<usize>().ok())
            .unwrap_or(0);
        request.len() >= end + 4 + length
    }

    #[test]
    fn an_answer_arrives_piece_by_piece_and_an_error_is_named() {
        let pieces = "HTTP/1.0 200 OK\r\nContent-Type: application/x-ndjson\r\n\r\n\
            {\"message\":{\"content\":\"Hola\"},\"done\":false}\n\
            {\"message\":{\"content\":\", soy Gemma.\"},\"done\":false}\n\
            {\"message\":{\"content\":\"\"},\"done\":true}\n";
        let mut seen = Vec::new();
        let answer = chat_at(serve(pieces), |piece| seen.push(piece.to_owned())).unwrap();
        assert_eq!(answer, "Hola, soy Gemma.");
        assert_eq!(seen, ["Hola", ", soy Gemma."]);

        let missing = "HTTP/1.0 404 Not Found\r\n\r\n{\"error\":\"model 'x' not found\"}";
        let error = chat_at(serve(missing), |_| {}).unwrap_err();
        assert_eq!(error.to_string(), "model 'x' not found");
    }

    /// This computer's Ollama, when it runs: `cargo test -- --ignored assistant`.
    #[test]
    #[ignore = "needs Ollama running with a local model"]
    fn this_computers_ollama_answers_piece_by_piece() {
        let models = models().unwrap();
        let view = View {
            models,
            ..View::default()
        };
        let model = view.model(None).expect("a local model");
        let started = std::time::Instant::now();
        let mut first = None;
        let mut pieces = 0;
        let message = json!({ "role": "user", "content": "Preséntate en una frase corta." });
        let answer = chat(model, vec![message], |_| {
            first.get_or_insert(started.elapsed());
            pieces += 1;
        })
        .unwrap();
        eprintln!(
            "{}: {pieces} pieces, first after {first:?}, all after {:?}: {answer}",
            model.name,
            started.elapsed()
        );
        assert!(pieces > 1 && !answer.trim().is_empty());
    }

    /// Suggest and Answer on this computer's Ollama, warm: how soon each
    /// starts and what it says. `cargo test -- --ignored suggest_and_answer --nocapture`
    #[test]
    #[ignore = "needs Ollama running with a local model"]
    fn suggest_and_answer_on_this_computers_ollama() {
        let view = View {
            models: models().unwrap(),
            ..View::default()
        };
        let model = view.model(None).expect("a local model").clone();
        let documents = [(
            "Script".to_owned(),
            "## Sobre mí
- Backend, 7 años, Python y Go
- Lidero un equipo de 4
             ## Historias
### Migración de pagos a colas
- 2025, errores -80 %, sin caídas"
                .to_owned(),
        )];
        let conversation =
            "Others: Gracias por venir. Cuéntame un proyecto del que estés orgulloso.
             Me: Claro.
Others: Y luego, ¿cómo invertirías una lista enlazada?";
        let _ = chat(
            &model,
            vec![json!({ "role": "user", "content": "hola" })],
            |_| {},
        );
        let question = Some("¿cómo invertirías una lista enlazada?");
        for action in [Action::Suggest, Action::Answer, Action::Hint] {
            let question = question.filter(|_| action == Action::Hint);
            let messages = crate::teleprompter::prompts::messages(
                action,
                "es",
                conversation,
                question,
                &documents,
            );
            let started = std::time::Instant::now();
            let mut first = None;
            let answer = chat(&model, messages, |_| {
                first.get_or_insert(started.elapsed());
            })
            .unwrap();
            eprintln!(
                "=== {} on {}: first after {first:?}, all after {:?}
{answer}
",
                action.label(),
                model.name,
                started.elapsed()
            );
        }
    }

    /// [`chat`] against `address` instead of Ollama's port.
    fn chat_at(address: SocketAddr, piece: impl FnMut(&str)) -> io::Result<String> {
        ADDRESS_OVERRIDE.with(|it| it.set(Some(address)));
        let model = Model {
            name: "gemma4:12b".into(),
            thinks: true,
        };
        chat(
            &model,
            vec![json!({ "role": "user", "content": "hola" })],
            piece,
        )
    }
}
