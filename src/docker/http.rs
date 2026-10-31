//! The one way mogumogu talks to a Docker engine: a fixed `GET` over the
//! engine's local named pipe, answered as HTTP/1.1 with JSON. There is no
//! method parameter and no free path: only the read-only [`Query`] list can
//! be asked. The answer is bounded in size and time.
use crate::platform::EnginePipe;
use std::time::{Duration, Instant};

/// Larger answers are refused rather than cut. Parsed JSON takes several
/// times this in memory, so the bound stays modest.
const MAX_ANSWER: usize = 8 << 20;
const MAX_HEADERS: usize = 64 << 10;

/// The read-only endpoints of the Engine API mogumogu uses. Unversioned
/// paths take the engine's own API version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Query {
    Version,
    Containers,
    Images,
    Volumes,
    Networks,
    /// `docker system df`; can take a while on large stores.
    DiskUsage,
}

impl Query {
    fn path(self) -> &'static str {
        match self {
            Self::Version => "/version",
            Self::Containers => "/containers/json?all=1",
            Self::Images => "/images/json",
            Self::Volumes => "/volumes",
            Self::Networks => "/networks",
            Self::DiskUsage => "/system/df",
        }
    }
}

/// The exact bytes sent for `query`.
pub(super) fn request(query: Query) -> Vec<u8> {
    format!(
        "GET {} HTTP/1.1\r\nHost: docker\r\nUser-Agent: mogumogu\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        query.path()
    )
    .into_bytes()
}

/// Asks the engine behind `pipe` one question and returns its JSON.
pub fn get(pipe: &str, query: Query, timeout: Duration) -> Result<serde_json::Value, String> {
    let deadline = Instant::now() + timeout;
    let mut engine = EnginePipe::connect(pipe, deadline).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "Docker läuft nicht (Pipe nicht vorhanden)".to_string(),
        _ => format!("Docker-Pipe nicht erreichbar: {e}"),
    })?;
    engine.send(&request(query), deadline).map_err(|e| format!("Anfrage an Docker: {e}"))?;
    let mut answer = Vec::new();
    let mut buffer = vec![0_u8; 64 << 10];
    let mut framing = None;
    loop {
        // The head is read once; the body is decoded once it can be complete.
        if framing.is_none() {
            framing = head(&answer)?;
        }
        if let Some(framing) = &framing
            && framing.may_be_complete(&answer)
            && let Some(body) = framing.body(&answer)?
        {
            return serde_json::from_slice(&body).map_err(|e| format!("Docker-Antwort ist kein JSON: {e}"));
        }
        if Instant::now() >= deadline {
            return Err("Docker antwortet nicht rechtzeitig".into());
        }
        let read = engine.receive(&mut buffer, deadline).map_err(|e| format!("Antwort von Docker: {e}"))?;
        if read == 0 {
            return Err("Docker hat die Verbindung vor dem Ende der Antwort geschlossen".into());
        }
        if answer.len() + read > MAX_ANSWER {
            return Err(format!("Antwort von Docker größer als {} MB", MAX_ANSWER >> 20));
        }
        answer.extend_from_slice(&buffer[..read]);
    }
}

/// How the body of an answer ends.
#[derive(Debug)]
pub(super) struct Framing {
    start: usize,
    /// `None` for a chunked body.
    length: Option<usize>,
}

impl Framing {
    /// Cheap test before decoding: the length arrived, or a chunked body ends
    /// with its last chunk.
    fn may_be_complete(&self, answer: &[u8]) -> bool {
        match self.length {
            Some(length) => answer.len() >= self.start + length,
            None => answer[self.start..].ends_with(b"0\r\n\r\n"),
        }
    }

    fn body(&self, answer: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let body = &answer[self.start..];
        match self.length {
            Some(length) => Ok((body.len() >= length).then(|| body[..length].to_vec())),
            None => chunked(body),
        }
    }
}

/// The body of a complete `200` answer, `None` while it is incomplete.
#[cfg(test)]
pub(super) fn parse(answer: &[u8]) -> Result<Option<Vec<u8>>, String> {
    match head(answer)? {
        Some(framing) => framing.body(answer),
        None => Ok(None),
    }
}

/// The framing of a `200` answer once its head is complete.
pub(super) fn head(answer: &[u8]) -> Result<Option<Framing>, String> {
    let Some(end) = answer.windows(4).position(|w| w == b"\r\n\r\n") else {
        return if answer.len() > MAX_HEADERS { Err("Kopfzeilen von Docker zu lang".into()) } else { Ok(None) };
    };
    let head = std::str::from_utf8(&answer[..end]).map_err(|_| "Kopfzeilen von Docker nicht lesbar".to_string())?;
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or_default();
    let code = status.split(' ').nth(1).unwrap_or_default();
    if !status.starts_with("HTTP/1.") || code != "200" {
        return Err(format!("Docker antwortet „{}“", status.chars().take(80).collect::<String>()));
    }
    let header = |name: &str| {
        head.split("\r\n").skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim().eq_ignore_ascii_case(name).then(|| value.trim().to_ascii_lowercase())
        })
    };
    let start = end + 4;
    if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        return Ok(Some(Framing { start, length: None }));
    }
    match header("content-length") {
        Some(length) => {
            let length: usize = length.parse().map_err(|_| "Ungültige Content-Length von Docker".to_string())?;
            if length > MAX_ANSWER {
                return Err("Antwort von Docker zu groß".into());
            }
            Ok(Some(Framing { start, length: Some(length) }))
        }
        None => Err("Antwort von Docker ohne Längenangabe".into()),
    }
}

/// Decodes a chunked body; `None` until the last chunk arrived.
fn chunked(mut body: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut out = Vec::new();
    loop {
        let Some(line) = body.windows(2).position(|w| w == b"\r\n") else { return Ok(None) };
        let size = std::str::from_utf8(&body[..line]).ok().and_then(|s| s.split(';').next());
        let size = size
            .and_then(|s| usize::from_str_radix(s.trim(), 16).ok())
            .ok_or_else(|| "Ungültiger Abschnitt in der Antwort von Docker".to_string())?;
        if size > MAX_ANSWER || out.len() + size > MAX_ANSWER {
            return Err("Antwort von Docker zu groß".into());
        }
        let data = line + 2;
        if size == 0 {
            return Ok(Some(out));
        }
        if body.len() < data + size + 2 {
            return Ok(None);
        }
        out.extend_from_slice(&body[data..data + size]);
        body = &body[data + size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_request_is_a_get_of_a_fixed_path() {
        for query in
            [Query::Version, Query::Containers, Query::Images, Query::Volumes, Query::Networks, Query::DiskUsage]
        {
            let text = String::from_utf8(request(query)).unwrap();
            assert!(text.starts_with("GET /") && text.ends_with("\r\n\r\n"), "{text}");
            assert_eq!(text.matches("\r\n\r\n").count(), 1);
        }
    }

    #[test]
    fn answers_are_read_by_length_or_in_chunks() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}";
        assert_eq!(parse(plain).unwrap().unwrap(), b"{\"a\":1}");
        assert_eq!(parse(&plain[..plain.len() - 1]).unwrap(), None, "waits for the rest");
        let chunks = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n[1,\r\n2\r\n2]\r\n0\r\n\r\n";
        assert_eq!(parse(chunks).unwrap().unwrap(), b"[1,2]");
        assert_eq!(parse(&chunks[..chunks.len() - 7]).unwrap(), None);
        assert!(parse(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n").is_err());
        assert!(parse(b"HTTP/1.1 200 OK\r\nContent-Length: 99999999999\r\n\r\n").is_err());
    }
}
