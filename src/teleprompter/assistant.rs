//! MicroPrompt's local model (Labs › Assistant): Ollama on this computer,
//! over loopback HTTP, so nothing leaves it. Ollama's cloud models are left
//! out. [`chat`] is the client the assistant's actions use; [`view`],
//! [`check`] and [`try_it`] are what the Teleprompter pane shows.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

/// Ollama's own port, on this computer only.
const ADDRESS: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 11434);
/// EN-01's pick: the shortest, clearest points that keep to the script.
pub(crate) const DEFAULT_MODEL: &str = "gemma4:12b";
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
    let mut body = json!({ "model": model.name, "messages": messages, "stream": true });
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
    /// The test answer so far.
    pub answer: String,
    pub asking: bool,
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
    answer: String::new(),
    asking: false,
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

/// Asks `model` `question`, the answer streaming into [`View::answer`].
pub(crate) fn try_it(model: Model, question: String) {
    if lock().asking {
        return;
    }
    update(|view| {
        view.asking = true;
        view.answer.clear();
        view.error = None;
    });
    std::thread::spawn(move || {
        let message = json!({ "role": "user", "content": question });
        let result = chat(&model, vec![message], |piece| {
            update(|view| view.answer.push_str(piece));
        });
        update(|view| {
            view.asking = false;
            match result {
                Ok(_) => {}
                Err(error) if not_running(&error) => view.status = Status::NotRunning,
                Err(error) => view.error = Some(error.to_string()),
            }
        });
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
