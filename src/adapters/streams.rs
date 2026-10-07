use std::fmt;
use std::io::{BufRead, Read};

use schemars::JsonSchema;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use super::sources::{MetadataOverrides, read_utf8};
use crate::app::ingest::InputItem;
use crate::domain::documents::{DocumentInput, TemporalState};
use crate::{CommonplaceError, Result};

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = temporal_fields)]
pub struct Record {
    #[schemars(length(min = 1), regex(pattern = "^[^\\u0000]+$"))]
    pub source_key: String,
    pub text: String,
    pub title: Option<String>,
    #[serde(default = "text_source_type")]
    #[schemars(length(min = 1), regex(pattern = "^[^\\u0000]+$"))]
    pub source_type: String,
    pub temporal_state: TemporalState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurred_at: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

fn temporal_fields(schema: &mut schemars::Schema) {
    schema.insert("allOf".into(), serde_json::json!([{
        "if": {"required": ["temporal_state"], "properties": {"temporal_state": {"const": "dated"}}},
        "then": {"required": ["occurred_at"], "properties": {"occurred_at": {"type": "string", "format": "date-time"}}},
        "else": {"not": {"required": ["occurred_at"]}}
    }]));
}

fn text_source_type() -> String {
    "text".into()
}

impl From<Record> for DocumentInput {
    fn from(record: Record) -> Self {
        Self {
            source_key: record.source_key,
            text: record.text,
            title: record.title,
            source_type: record.source_type,
            temporal_state: record.temporal_state,
            occurred_at: record.occurred_at,
            metadata: record.metadata,
        }
    }
}

pub fn stdin_item(
    reader: impl Read,
    source_key: String,
    metadata: &MetadataOverrides,
    maximum_bytes: usize,
) -> InputItem {
    InputItem {
        input: "stdin".into(),
        source_key: Some(source_key.clone()),
        document: read_utf8(reader, maximum_bytes)
            .map_err(|error| error.context("ingest stdin"))
            .map(|text| DocumentInput {
                source_key,
                text,
                title: metadata.title.clone(),
                source_type: metadata.source_type.clone(),
                temporal_state: metadata.temporal_state,
                occurred_at: metadata.occurred_at.clone(),
                metadata: metadata.metadata.clone(),
            }),
    }
}

/// Reject duplicate object keys before converting to serde_json's map representation.
struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON without duplicate object fields")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom(format!("duplicate field {key:?}")));
                    }
                    values.insert(key, map.next_value::<UniqueValue>()?.0);
                }
                Ok(UniqueValue(Value::Object(values)))
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<UniqueValue>()? {
                    values.push(value.0);
                }
                Ok(UniqueValue(Value::Array(values)))
            }

            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(value.into())))
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(value)))
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }

            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

fn invalid(error: impl fmt::Display) -> CommonplaceError {
    CommonplaceError::InvalidInput(error.to_string())
}

pub fn parse_record(bytes: &[u8]) -> Result<Record> {
    let value = serde_json::from_slice::<UniqueValue>(bytes)
        .map_err(invalid)?
        .0;
    let schema = record_schema();
    let validator = jsonschema::validator_for(schema.as_value()).map_err(|error| {
        CommonplaceError::Storage(format!("invalid generated record schema: {error}"))
    })?;
    validator
        .validate(&value)
        .map_err(|error| CommonplaceError::validation(&error))?;
    serde_json::from_value(value).map_err(invalid)
}

pub fn record_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<Record>()
}

pub struct JsonLines<R> {
    reader: R,
    label: String,
    maximum_bytes: usize,
    maximum_documents: usize,
    line: usize,
    done: bool,
}

impl<R: BufRead> JsonLines<R> {
    pub fn new(reader: R, label: String, maximum_bytes: usize, maximum_documents: usize) -> Self {
        Self {
            reader,
            label,
            maximum_bytes,
            maximum_documents,
            line: 0,
            done: false,
        }
    }

    fn read_line(&mut self) -> Result<Option<Vec<u8>>> {
        let mut bytes = Vec::new();
        loop {
            let available = self.reader.fill_buf()?;
            if available.is_empty() {
                if bytes.len() > self.maximum_bytes {
                    return Err(self.oversized());
                }
                return Ok((!bytes.is_empty()).then_some(bytes));
            }
            let remaining = self.maximum_bytes.saturating_add(2) - bytes.len();
            let available = &available[..available.len().min(remaining)];
            let newline = available.iter().position(|byte| *byte == b'\n');
            let count = newline.map_or(available.len(), |position| position + 1);
            bytes.extend_from_slice(&available[..count]);
            self.reader.consume(count);
            if newline.is_some() {
                bytes.pop();
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                if bytes.len() > self.maximum_bytes {
                    return Err(self.oversized());
                }
                return Ok(Some(bytes));
            }
            if bytes.len() > self.maximum_bytes
                && (bytes.len() > self.maximum_bytes.saturating_add(1)
                    || bytes.last() != Some(&b'\r'))
            {
                return Err(self.oversized());
            }
        }
    }

    fn oversized(&self) -> CommonplaceError {
        CommonplaceError::LimitExceeded(format!(
            "JSON record exceeds the {}-byte limit; remaining input was not processed",
            self.maximum_bytes
        ))
    }
}

impl<R: BufRead> Iterator for JsonLines<R> {
    type Item = InputItem;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let result = if self.line >= self.maximum_documents {
            self.done = true;
            match self.reader.fill_buf() {
                Ok([]) => return None,
                Ok(_) => Err(CommonplaceError::LimitExceeded(format!(
                    "request exceeds the {}-document limit; remaining input was not processed",
                    self.maximum_documents
                ))),
                Err(error) => Err(error.into()),
            }
        } else {
            self.read_line()
        };
        self.line += 1;
        let input = format!("{}:{}", self.label, self.line);
        match result {
            Ok(None) => {
                self.done = true;
                None
            }
            Ok(Some(bytes)) => match parse_record(&bytes) {
                Ok(record) => Some(InputItem {
                    input,
                    source_key: Some(record.source_key.clone()),
                    document: Ok(record.into()),
                }),
                Err(error) => Some(InputItem {
                    input: input.clone(),
                    source_key: None,
                    document: Err(error.context(format!("ingest JSONL {input}"))),
                }),
            },
            Err(error) => {
                self.done = true;
                Some(InputItem {
                    input: input.clone(),
                    source_key: None,
                    document: Err(error.context(format!("cannot read ingest JSONL {input}"))),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const RECORD: &[u8] = br#"{"source_key":"a","text":"","temporal_state":"unknown"}"#;

    #[test]
    fn physical_line_limits_are_inclusive_with_exact_delimiters() {
        for ending in [b"".as_slice(), b"\n", b"\r\n"] {
            for limit in [RECORD.len() - 1, RECORD.len(), RECORD.len() + 1] {
                let mut cursor = Cursor::new([RECORD, ending].concat());
                let mut lines = JsonLines::new(&mut cursor, "stdin".into(), limit, 3);
                let item = lines.next().unwrap();
                if limit < RECORD.len() {
                    assert_eq!(item.document.unwrap_err().code(), "limit_exceeded");
                } else {
                    assert_eq!(item.document.unwrap().source_key, "a");
                }
                assert!(lines.next().is_none());
                assert!(cursor.position() <= (limit + 2) as u64);
            }
            for capacity in [1, 2, 7] {
                let reader = std::io::BufReader::with_capacity(
                    capacity,
                    Cursor::new([RECORD, ending].concat()),
                );
                let mut lines = JsonLines::new(reader, "stdin".into(), RECORD.len(), 1);
                assert!(lines.next().unwrap().document.is_ok());
                assert!(lines.next().is_none());
            }
        }
        let mut lines = JsonLines::new(
            Cursor::new([RECORD, b"\r"].concat()),
            "stdin".into(),
            RECORD.len(),
            3,
        );
        assert_eq!(
            lines.next().unwrap().document.unwrap_err().code(),
            "limit_exceeded"
        );
        let mut lines = JsonLines::new(
            Cursor::new([RECORD, b"\r"].concat()),
            "stdin".into(),
            RECORD.len() + 1,
            3,
        );
        assert!(lines.next().unwrap().document.is_ok());
    }

    #[test]
    fn malformed_records_continue_but_oversized_records_never_drain() {
        let mut cursor = Cursor::new([RECORD, b"\n\n{broken}\n", RECORD, b"\n"].concat());
        let mut lines = JsonLines::new(&mut cursor, "in".into(), 100, 10);
        assert!(lines.next().unwrap().document.is_ok());
        for number in [2, 3] {
            let item = lines.next().unwrap();
            assert_eq!(item.input, format!("in:{number}"));
            assert_eq!(item.document.unwrap_err().code(), "invalid_input");
        }
        assert!(lines.next().unwrap().document.is_ok());
        assert!(lines.next().is_none());

        let mut cursor = Cursor::new(vec![b'x'; 10000]);
        let mut lines = JsonLines::new(&mut cursor, "in".into(), 16, 3);
        assert_eq!(
            lines.next().unwrap().document.unwrap_err().code(),
            "limit_exceeded"
        );
        assert!(lines.next().is_none());
        assert!(cursor.position() <= 18);
    }

    #[test]
    fn document_cap_probes_without_consuming_or_parsing_another_record() {
        let mut cursor = Cursor::new([RECORD, b"\n", &vec![b'x'; 10000]].concat());
        let mut lines = JsonLines::new(&mut cursor, "in".into(), 100, 1);
        assert!(lines.next().unwrap().document.is_ok());
        assert_eq!(
            lines.next().unwrap().document.unwrap_err().code(),
            "limit_exceeded"
        );
        assert!(lines.next().is_none());
        assert_eq!(cursor.position(), (RECORD.len() + 1) as u64);
    }

    #[test]
    fn strict_records_preserve_decoded_text_and_reject_duplicate_fields_at_any_depth() {
        let record = parse_record(br#"{"source_key":" A ","text":"\uFEFFa\r\n\u0000e\u0301\uD83E\uDD80","temporal_state":"unknown","metadata":{"a":[null,true,1]}}"#).unwrap();
        let document = DocumentInput::from(record).normalize().unwrap();
        assert_eq!(document.source_key, " A ");
        assert_eq!(document.text, "\u{feff}a\r\n\0e\u{301}\u{1f980}");
        assert_eq!(document.source_type, "text");
        for bytes in [
            br#"{"source_key":"a","text":"","temporal_state":"unknown","extra":1}"#.as_slice(),
            br#"{"source_key":"a","source_key":"b","text":"","temporal_state":"unknown"}"#,
            br#"{"source_key":"a","text":"","temporal_state":"unknown","metadata":{"x":1,"x":2}}"#,
            br#"{"source_key":"a","text":"","temporal_state":"unknown","metadata":{"x":[{"a":1,"a":2}]}}"#,
            br#"{"source_key":"","text":"","temporal_state":"unknown"}"#,
            br#"{"source_key":"a\u0000","text":"","temporal_state":"unknown"}"#,
            br#"{"source_key":"a","text":"","temporal_state":"unknown","source_type":null}"#,
            br#"{"source_key":"a","text":"","temporal_state":"unknown","metadata":null}"#,
            b"\xff",
        ] {
            assert_eq!(
                parse_record(bytes).unwrap_err().code(),
                "invalid_input",
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn raw_stdin_reads_only_source_limit_plus_one() {
        let metadata = MetadataOverrides {
            source_type: "text".into(),
            temporal_state: TemporalState::Unknown,
            title: None,
            occurred_at: None,
            metadata: Map::new(),
        };
        let mut cursor = Cursor::new(vec![b'x'; 1000]);
        let item = stdin_item(&mut cursor, "key".into(), &metadata, 10);
        assert_eq!(item.source_key.as_deref(), Some("key"));
        assert_eq!(item.document.unwrap_err().code(), "limit_exceeded");
        assert_eq!(cursor.position(), 11);
        let item = stdin_item(&b"\xff"[..], "key".into(), &metadata, 10);
        assert_eq!(item.document.unwrap_err().code(), "invalid_input");
    }

    #[test]
    fn stream_runtime_errors_keep_stdin_or_file_line_context() {
        struct Failed;
        impl Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from_raw_os_error(5))
            }
        }
        impl BufRead for Failed {
            fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
                Err(std::io::Error::from_raw_os_error(5))
            }
            fn consume(&mut self, _: usize) {}
        }
        let metadata = MetadataOverrides {
            source_type: "text".into(),
            temporal_state: TemporalState::Unknown,
            title: None,
            occurred_at: None,
            metadata: Map::new(),
        };
        let error = stdin_item(Failed, "key".into(), &metadata, 1024)
            .document
            .unwrap_err();
        assert_eq!(error.code(), "internal_error");
        assert_eq!(error.exit_code(), 1);
        assert!(error.to_string().contains("stdin"));
        for label in ["stdin", "records.jsonl"] {
            let mut items = JsonLines::new(Failed, label.into(), 1024, 10);
            let error = items.next().unwrap().document.unwrap_err();
            assert_eq!(error.code(), "internal_error");
            assert!(error.to_string().contains(&format!("{label}:1")));
            assert!(items.next().is_none());
        }
    }
}
