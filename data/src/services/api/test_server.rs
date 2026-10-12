//! A scripted local HTTP server for tests that make several requests in a
//! row (the one-request helper is `subsonic::tests::one_shot_server`).
//!
//! Every request is recorded and answered by the test's handler, one
//! connection per request (`Connection: close`).

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// One request the server saw.
#[derive(Debug, Clone)]
pub(crate) struct Recorded {
    pub method: String,
    /// The path without the query string.
    pub path: String,
    pub query: String,
    /// Header names lowercased, in order.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    /// A form or query parameter's value: the body's first (a form POST),
    /// else the query's.
    pub fn param(&self, name: &str) -> Option<String> {
        url::form_urlencoded::parse(self.body.as_bytes())
            .chain(url::form_urlencoded::parse(self.query.as_bytes()))
            .find_map(|(key, value)| (key == name).then(|| value.into_owned()))
    }

    /// A form parameter from the body only: a credential nokkvi POSTs must
    /// stay out of the URL, where server access logs keep it.
    pub fn form(&self, name: &str) -> Option<String> {
        url::form_urlencoded::parse(self.body.as_bytes())
            .find_map(|(key, value)| (key == name).then(|| value.into_owned()))
    }

    /// A header's value, by case-insensitive name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
    }

    /// The body as JSON (a native-API request).
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }

    pub fn is(&self, method: &str, path: &str) -> bool {
        self.method == method && self.path == path
    }
}

type Handler = dyn Fn(&Recorded) -> (u16, String) + Send + Sync;

pub(crate) struct ScriptedServer {
    pub base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for ScriptedServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ScriptedServer {
    /// Start a server on an ephemeral local port that answers each request
    /// with `handler`'s `(status, JSON body)`.
    pub(crate) async fn start(
        handler: impl Fn(&Recorded) -> (u16, String) + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let seen = requests.clone();
        let task = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let Some(request) = read_request(&mut socket).await else {
                    continue;
                };
                let (status, body) = handler(&request);
                seen.lock().push(request);
                let response = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    reason(status),
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        Self {
            base_url,
            requests,
            task,
        }
    }

    /// Every request so far, oldest first.
    pub(crate) fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().clone()
    }
}

/// Read one request: the head, then `Content-Length` bytes of body.
async fn read_request(socket: &mut tokio::net::TcpStream) -> Option<Recorded> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head_end, length) = loop {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        raw.extend_from_slice(&chunk[..read]);
        let text = String::from_utf8_lossy(&raw);
        if let Some(head_end) = text.find("\r\n\r\n") {
            let length = text[..head_end]
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            break (head_end, length);
        }
    };
    while raw.len() < head_end + 4 + length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&chunk[..read]);
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let headers = text[..head_end.min(text.len())]
        .lines()
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let mut first = text.lines().next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let (path, query) = target.split_once('?').map_or_else(
        || (target.clone(), String::new()),
        |(p, q)| (p.to_owned(), q.to_owned()),
    );
    let body_start = (head_end + 4).min(text.len());
    let body_end = (head_end + 4 + length).min(text.len());
    Some(Recorded {
        method,
        path,
        query,
        headers,
        body: text[body_start..body_end].to_owned(),
    })
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Internal Server Error",
    }
}

/// An ok Subsonic envelope around `inner` (a JSON object's fields, or empty).
pub(crate) fn subsonic_ok(inner: &str) -> String {
    let sep = if inner.is_empty() { "" } else { "," };
    format!(
        r#"{{"subsonic-response":{{"status":"ok","version":"1.16.1","type":"navidrome","openSubsonic":true{sep}{inner}}}}}"#
    )
}

/// A failed Subsonic envelope with `code`.
pub(crate) fn subsonic_failed(code: i64, message: &str) -> String {
    format!(
        r#"{{"subsonic-response":{{"status":"failed","version":"1.16.1","error":{{"code":{code},"message":"{message}"}}}}}}"#
    )
}

/// The `getOpenSubsonicExtensions` answer advertising `names`.
pub(crate) fn extensions(names: &[&str]) -> String {
    let list: Vec<String> = names
        .iter()
        .map(|name| format!(r#"{{"name":"{name}","versions":[1]}}"#))
        .collect();
    subsonic_ok(&format!(r#""openSubsonicExtensions":[{}]"#, list.join(",")))
}
