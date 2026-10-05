use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

use super::ids::{DocumentId, PassageId, RevisionId};
use crate::{CommonplaceError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemporalState {
    Dated,
    Timeless,
    Unknown,
}

impl TemporalState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dated => "dated",
            Self::Timeless => "timeless",
            Self::Unknown => "unknown",
        }
    }

    pub fn validate(self, occurred_at: Option<&str>) -> Result<()> {
        match (self, occurred_at) {
            (Self::Dated, None) => Err(CommonplaceError::InvalidInput(
                "dated temporal_state requires occurred_at (CLI: --occurred-at) as an RFC3339 timestamp".into(),
            )),
            (Self::Timeless | Self::Unknown, Some(_)) => Err(CommonplaceError::InvalidInput(
                format!("{} temporal_state prohibits occurred_at; omit the timestamp or choose dated", self.as_str()),
            )),
            _ => Ok(()),
        }
    }
}

impl std::str::FromStr for TemporalState {
    type Err = CommonplaceError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "dated" => Ok(Self::Dated),
            "timeless" => Ok(Self::Timeless),
            "unknown" => Ok(Self::Unknown),
            _ => Err(CommonplaceError::InvalidInput(
                "temporal_state must be dated (known event time), timeless (event time does not apply), or unknown (event time unavailable)".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentInput {
    pub source_key: String,
    pub text: String,
    pub title: Option<String>,
    pub source_type: String,
    pub temporal_state: TemporalState,
    pub occurred_at: Option<String>,
    pub metadata: Map<String, Value>,
}

impl DocumentInput {
    pub fn normalize(mut self) -> Result<Self> {
        if self.source_key.is_empty() || self.source_key.contains('\0') {
            return Err(CommonplaceError::InvalidInput(
                "source_key must be nonempty and contain no NUL".into(),
            ));
        }
        if self.source_type.is_empty() || self.source_type.contains('\0') {
            return Err(CommonplaceError::InvalidInput(
                "source_type must be nonempty and contain no NUL".into(),
            ));
        }
        self.temporal_state.validate(self.occurred_at.as_deref())?;
        self.occurred_at = self
            .occurred_at
            .as_deref()
            .map(normalize_timestamp)
            .transpose()?;
        canonical_json(&Value::Object(self.metadata.clone()))?;
        Ok(self)
    }

    pub fn metadata_json(&self) -> Result<String> {
        canonical_json(&Value::Object(self.metadata.clone()))
    }

    pub fn revision_digest(&self) -> Result<String> {
        fn field(hash: &mut Sha256, value: &str) {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        }
        fn optional(hash: &mut Sha256, value: Option<&str>) {
            hash.update([u8::from(value.is_some())]);
            if let Some(value) = value {
                field(hash, value);
            }
        }
        let mut hash = Sha256::new();
        hash.update(b"commonplace-revision/2\0");
        field(&mut hash, &self.text);
        optional(&mut hash, self.title.as_deref());
        field(&mut hash, &self.source_type);
        field(&mut hash, self.temporal_state.as_str());
        optional(&mut hash, self.occurred_at.as_deref());
        field(&mut hash, &self.metadata_json()?);
        Ok(format!("{:x}", hash.finalize()))
    }
}

pub fn normalize_timestamp(value: &str) -> Result<String> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| CommonplaceError::InvalidInput(format!("invalid occurred_at: {error}")))?
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .map_err(|error| CommonplaceError::InvalidInput(format!("invalid occurred_at: {error}")))
}

pub fn canonical_json(value: &Value) -> Result<String> {
    fn write(value: &Value, output: &mut String) -> Result<()> {
        match value {
            Value::Number(number) if number.as_i64().is_none() => {
                return Err(CommonplaceError::InvalidInput(
                    "metadata numbers must be signed 64-bit integers; floats are not supported"
                        .into(),
                ));
            }
            Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    write(value, output)?;
                }
                output.push(']');
            }
            Value::Object(values) => {
                output.push('{');
                for (index, (key, value)) in
                    values.iter().collect::<BTreeMap<_, _>>().iter().enumerate()
                {
                    if index != 0 {
                        output.push(',');
                    }
                    output.push_str(&serde_json::to_string(key)?);
                    output.push(':');
                    write(value, output)?;
                }
                output.push('}');
            }
            _ => output.push_str(&serde_json::to_string(value)?),
        }
        Ok(())
    }
    let mut output = String::new();
    write(value, &mut output)?;
    Ok(output)
}

#[derive(Debug, Serialize)]
pub struct Document {
    pub document_id: DocumentId,
    pub source_key: String,
    pub created_at: String,
    pub last_ingested_at: String,
    pub current_revision_id: RevisionId,
    pub revision_ids: Vec<RevisionId>,
}

#[derive(Debug, Serialize)]
pub struct DocumentRevision {
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
    pub source_key: String,
    pub revision_number: i64,
    pub revision_digest: String,
    pub text: String,
    pub title: Option<String>,
    pub source_type: String,
    pub temporal_state: TemporalState,
    pub occurred_at: Option<String>,
    pub metadata: Map<String, Value>,
    pub created_at: String,
    pub passage_ids: Vec<PassageId>,
}

#[derive(Debug, Serialize)]
pub struct Evidence {
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
    pub passage_id: PassageId,
    pub source_key: String,
    pub revision_number: i64,
    pub ordinal: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub text: String,
    pub title: Option<String>,
    pub source_type: String,
    pub temporal_state: TemporalState,
    pub occurred_at: Option<String>,
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStatus {
    Added,
    Updated,
    Unchanged,
}

#[derive(Debug)]
pub struct Publication {
    pub status: PublicationStatus,
    pub document_id: DocumentId,
    pub revision_id: RevisionId,
    pub passage_ids: Vec<PassageId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_golden_and_number_rejection() {
        let value = json!({"é": "\u{feff}\0\n\"\\🦀", "a": [null, true, {"z": i64::MAX, "a": i64::MIN}], "Z": 0});
        assert_eq!(
            canonical_json(&value).unwrap(),
            "{\"Z\":0,\"a\":[null,true,{\"a\":-9223372036854775808,\"z\":9223372036854775807}],\"é\":\"\u{feff}\\u0000\\n\\\"\\\\🦀\"}"
        );
        for value in [
            json!(0.0),
            json!(-0.0),
            json!(1e10),
            json!(u64::MAX),
            json!({"nested": [1.5]}),
        ] {
            assert_eq!(canonical_json(&value).unwrap_err().code(), "invalid_input");
        }
    }

    #[test]
    fn digest_presence_boundaries_and_metadata_are_significant() {
        let input = DocumentInput {
            source_key: "not-in-digest".into(),
            text: "\u{feff}A\r\n\0e\u{301}🦀".into(),
            title: None,
            source_type: "file".into(),
            temporal_state: TemporalState::Dated,
            occurred_at: Some("2026-01-02T03:04:05+02:00".into()),
            metadata: json!({"z": [true, null], "a": i64::MIN})
                .as_object()
                .unwrap()
                .clone(),
        }
        .normalize()
        .unwrap();
        assert_eq!(input.occurred_at.as_deref(), Some("2026-01-02T01:04:05Z"));
        let digest = input.revision_digest().unwrap();
        assert_eq!(
            digest,
            "da4b5982cfe0eac1a536247e06c02060faaf450e6032ded155f5baac1df5e400"
        );
        let mut changed = input.clone();
        changed.source_key = "other-key".into();
        assert_eq!(changed.revision_digest().unwrap(), digest);
        changed.title = Some(String::new());
        assert_ne!(changed.revision_digest().unwrap(), digest);
        changed = input.clone();
        changed.text = changed.text.replace("\r\n", "\n");
        assert_ne!(changed.revision_digest().unwrap(), digest);
        changed = input.clone();
        changed.metadata.insert("new".into(), json!(true));
        assert_ne!(changed.revision_digest().unwrap(), digest);
        changed = input.clone();
        changed.occurred_at = None;
        changed.temporal_state = TemporalState::Timeless;
        let timeless = changed.revision_digest().unwrap();
        changed.temporal_state = TemporalState::Unknown;
        assert_ne!(changed.revision_digest().unwrap(), timeless);
        assert!(normalize_timestamp("not-time").is_err());
        assert_eq!(
            normalize_timestamp("2026-01-02T01:04:05.000Z").unwrap(),
            "2026-01-02T01:04:05Z"
        );
    }
}
