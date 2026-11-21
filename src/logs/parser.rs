//! An incremental Server-Sent Events parser.

use super::LogEvent;

/// An incremental SSE parser: feed it bytes, take complete events out.
#[derive(Default)]
pub(super) struct Parser {
    buf: String,
    pending: Vec<u8>,
    pub(super) closed: bool,
}

impl Parser {
    pub(super) fn push(&mut self, chunk: &[u8]) {
        // Hold back a UTF-8 sequence split across chunks.
        self.pending.extend_from_slice(chunk);
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(s) => s.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.pending.len(),
        };
        let text = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
        self.pending.drain(..valid);
        self.buf.push_str(&text.replace("\r\n", "\n"));
    }

    /// The next event with data, skipping comments and keep-alives.
    pub(super) fn next_event(&mut self) -> Option<Result<LogEvent, serde_json::Error>> {
        loop {
            let end = self.buf.find("\n\n")?;
            let block: String = self.buf.drain(..end + 2).collect();
            let mut id = None;
            let mut data = String::new();
            for line in block.lines() {
                if line.starts_with(':') {
                    continue;
                }
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "id" => id = Some(value.to_string()),
                    "data" => {
                        if !data.is_empty() {
                            data.push('\n');
                        }
                        data.push_str(value);
                    }
                    _ => {}
                }
            }
            if data.is_empty() {
                continue;
            }
            return Some(serde_json::from_str::<LogEvent>(&data).map(|mut e| {
                e.id = id;
                e
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::LogSource;

    #[test]
    fn parses_events_across_chunks() {
        let mut p = Parser::default();
        p.push(b": keep-alive\n\nid: 2026-06-01T12:02:03Z/000000000001\ndata: {\"ts\":\"2026-06-01T12:02:03Z\",\"source\":\"container\",");
        assert!(p.next_event().is_none());
        p.push(b"\"line\":\"Model loaded.\"}\r\n\r\nid: 2\ndata: {\"ts\":\"t\",\"source\":\"system\",\"line\":\"caf\xc3");
        let e = p.next_event().unwrap().unwrap();
        assert_eq!(e.id.as_deref(), Some("2026-06-01T12:02:03Z/000000000001"));
        assert_eq!(e.source, LogSource::Container);
        assert_eq!(e.line, "Model loaded.");
        assert!(p.next_event().is_none());
        p.push(b"\xa9\"}\n\n");
        let e = p.next_event().unwrap().unwrap();
        assert_eq!(e.line, "café");
        assert_eq!(e.source, LogSource::System);
    }

    #[test]
    fn a_bad_payload_is_an_error_not_the_end() {
        let mut p = Parser::default();
        p.push(b"data: nope\n\ndata: {\"ts\":\"t\",\"source\":\"x\",\"line\":\"l\"}\n\n");
        assert!(p.next_event().unwrap().is_err());
        let e = p.next_event().unwrap().unwrap();
        assert_eq!(e.source, LogSource::Other("x".into()));
    }
}
