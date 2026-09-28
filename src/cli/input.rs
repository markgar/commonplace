use std::io::Read;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::domain::schema::SchemaInput;
use crate::{CommonplaceError, Result};

#[derive(Debug, Serialize)]
pub struct Description {
    pub input_schema: schemars::Schema,
    pub example: Value,
}

pub(super) fn generated_schema<T: schemars::JsonSchema>() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<T>()
}

pub(super) fn validate(value: &Value, schema: &schemars::Schema) -> Result<()> {
    let validator = jsonschema::validator_for(schema.as_value()).map_err(|error| {
        CommonplaceError::Storage(format!("invalid generated input schema: {error}"))
    })?;
    validator.validate(value).map_err(|error| {
        CommonplaceError::InvalidInput(format!("{}: {error}", error.instance_path))
    })
}

pub fn describe() -> Result<Description> {
    let input_schema = generated_schema::<SchemaInput>();
    let example = serde_json::json!({"entity_types": [{"name": "person"}]});
    validate(&example, &input_schema)?;
    Ok(Description {
        input_schema,
        example,
    })
}

pub fn read(path: &Path, maximum_bytes: usize) -> Result<SchemaInput> {
    let file = std::fs::File::open(path)?;
    parse(file, maximum_bytes)
}

fn parse(reader: impl Read, maximum_bytes: usize) -> Result<SchemaInput> {
    let mut bytes = Vec::new();
    reader
        .take(maximum_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum_bytes {
        return Err(CommonplaceError::LimitExceeded(format!(
            "schema input exceeds the {maximum_bytes}-byte limit"
        )));
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))?;
    validate(&value, &generated_schema::<SchemaInput>())?;
    // Decode the original bytes so duplicate JSON fields are not silently collapsed.
    serde_json::from_slice(&bytes)
        .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn input_limit_is_inclusive_and_read_is_bounded() {
        for size in [15, 16, 17] {
            let input = format!("{{}}{}", " ".repeat(size - 2));
            let result = parse(input.as_bytes(), 16);
            if size <= 16 {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.unwrap_err().code(), "limit_exceeded");
            }
        }
        let mut reader = std::io::Cursor::new(vec![b' '; 100]);
        assert_eq!(parse(&mut reader, 16).unwrap_err().code(), "limit_exceeded");
        assert_eq!(reader.position(), 17);
    }
}
