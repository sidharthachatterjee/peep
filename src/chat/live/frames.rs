//! The framing of Google's long-lived channel: a stream of `<length>\n<JSON>` chunks, where
//! the length counts UTF-16 units and the JSON is an array of numbered arrays.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The array's number. The next request acknowledges the highest one seen.
    pub number: u64,
    pub payload: Payload,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    /// The channel's opening: its session id, and how often the server will send something.
    Opened {
        session: String,
        keepalive_ms: u64,
    },
    /// Nothing happened; the channel is alive.
    Noop,
    /// The server is closing the channel.
    Stop,
    /// What the channel carries.
    Data(Value),
    Other,
}

/// The stream did not follow the framing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Malformed;

/// Collects streamed bytes and gives out frames as they complete.
#[derive(Default)]
pub struct Reader {
    buffer: Vec<u8>,
}

impl Reader {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, Malformed> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();
        let mut start = 0;
        loop {
            let rest = &self.buffer[start..];
            let skipped = rest
                .iter()
                .take_while(|byte| byte.is_ascii_whitespace())
                .count();
            let rest = &rest[skipped..];
            let Some(newline) = rest.iter().position(|byte| *byte == b'\n') else {
                break;
            };
            let units: usize = std::str::from_utf8(&rest[..newline])
                .ok()
                .and_then(|length| length.parse().ok())
                .ok_or(Malformed)?;
            let body = &rest[newline + 1..];
            let Some(length) = byte_length(body, units) else {
                break;
            };
            frames.extend(parse(&body[..length])?);
            start += skipped + newline + 1 + length;
        }
        self.buffer.drain(..start);
        Ok(frames)
    }
}

/// How many bytes hold the first `units` UTF-16 units of `bytes`, or `None` if more bytes
/// are still to come.
fn byte_length(bytes: &[u8], units: usize) -> Option<usize> {
    let (mut index, mut counted) = (0, 0);
    while counted < units {
        let width = match *bytes.get(index)? {
            lead if lead < 0x80 => 1,
            lead if lead >= 0xf0 => 4,
            lead if lead >= 0xe0 => 3,
            _ => 2,
        };
        // A character outside the basic plane is four bytes here and two units there.
        counted += if width == 4 { 2 } else { 1 };
        index += width;
    }
    (index <= bytes.len()).then_some(index)
}

fn parse(chunk: &[u8]) -> Result<Vec<Frame>, Malformed> {
    let Ok(Value::Array(arrays)) = serde_json::from_slice(chunk) else {
        return Err(Malformed);
    };
    Ok(arrays
        .into_iter()
        .filter_map(|array| {
            let number = array.get(0)?.as_u64()?;
            let body = array.get(1)?.as_array()?;
            let payload = match body.first() {
                Some(Value::String(tag)) => match tag.as_str() {
                    "noop" => Payload::Noop,
                    "stop" | "close" => Payload::Stop,
                    "c" => match body.get(1).and_then(Value::as_str) {
                        Some(session) if !session.is_empty() => Payload::Opened {
                            session: session.to_owned(),
                            keepalive_ms: body.get(5).and_then(Value::as_u64).unwrap_or(30_000),
                        },
                        _ => Payload::Other,
                    },
                    _ => Payload::Other,
                },
                Some(Value::Array(_)) => Payload::Data(body[0].clone()),
                _ => Payload::Other,
            };
            Some(Frame { number, payload })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn chunk(json: &str) -> Vec<u8> {
        format!("{}\n{json}", json.encode_utf16().count()).into_bytes()
    }

    #[test]
    fn frames_come_out_as_their_bytes_arrive() {
        let mut bytes = chunk(r#"[[0,["c","SID","",8,15,30000]]]"#);
        bytes.extend(chunk(r#"[[1,["noop"]],[2,[[["1",[["100"]]]]]]]"#));
        let mut reader = Reader::default();

        let (first, second) = bytes.split_at(20);
        assert_eq!(reader.feed(first), Ok(vec![]), "the chunk is not whole yet");
        let frames = reader.feed(second).unwrap();

        assert_eq!(
            frames,
            [
                Frame {
                    number: 0,
                    payload: Payload::Opened {
                        session: "SID".into(),
                        keepalive_ms: 30_000
                    }
                },
                Frame {
                    number: 1,
                    payload: Payload::Noop
                },
                Frame {
                    number: 2,
                    payload: Payload::Data(json!([["1", [["100"]]]]))
                },
            ]
        );
        assert_eq!(reader.feed(b""), Ok(vec![]));
    }

    #[test]
    fn lengths_count_utf16_units_not_bytes() {
        // "é" is two bytes and one unit; the emoji is four bytes and two units.
        let mut bytes = chunk(r#"[[3,[["é😀"]]]]"#);
        bytes.extend(chunk(r#"[[4,["stop"]]]"#));
        let frames = Reader::default().feed(&bytes).unwrap();

        assert_eq!(frames[0].payload, Payload::Data(json!(["é😀"])));
        assert_eq!(
            frames[1],
            Frame {
                number: 4,
                payload: Payload::Stop
            }
        );
    }

    #[test]
    fn a_stream_that_is_not_framed_is_refused() {
        assert_eq!(Reader::default().feed(b"<html>\n"), Err(Malformed));
        assert_eq!(Reader::default().feed(b"5\n{...}"), Err(Malformed));
    }
}
