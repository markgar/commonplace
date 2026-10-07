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
    validator
        .validate(value)
        .map_err(|error| CommonplaceError::validation(&error))
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
    let file = open_file(path, "schema apply")?;
    parse(file, maximum_bytes)
        .map_err(|error| error.context(format!("schema apply input file {}", path.display())))
}

pub(super) fn open_file(path: &Path, operation: &str) -> Result<std::fs::File> {
    let context = format!("cannot open {operation} input file {}", path.display());
    let file =
        std::fs::File::open(path).map_err(|error| CommonplaceError::input_io(&context, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| CommonplaceError::input_io(&context, error))?;
    if metadata.is_dir() {
        return Err(CommonplaceError::InvalidInput(format!(
            "{operation} input path {} is a directory; supply a JSON file path",
            path.display()
        )));
    }
    Ok(file)
}

pub(super) fn read_file(path: &Path, maximum_bytes: usize, operation: &str) -> Result<Vec<u8>> {
    read_bounded(open_file(path, operation)?, maximum_bytes, operation)
        .map_err(|error| error.context(format!("{operation} input file {}", path.display())))
}

pub fn describe_scope() -> Result<Description> {
    let input_schema = generated_schema::<crate::domain::search::DocumentScope>();
    let example = serde_json::json!({"document_ids":["doc:1"],"truncated":false});
    validate(&example, &input_schema)?;
    Ok(Description {
        input_schema,
        example,
    })
}

pub fn read_scope(path: &Path) -> Result<crate::domain::search::DocumentScope> {
    let bytes = if path == Path::new("-") {
        read_bounded(
            std::io::stdin().lock(),
            crate::domain::search::MAX_SCOPE_BYTES,
            "--scope stdin",
        )
    } else {
        read_bounded(
            open_file(path, "--scope").map_err(|error| {
                error.context(
                    "--scope expects a JSON file path, not inline JSON; use --scope - for stdin",
                )
            })?,
            crate::domain::search::MAX_SCOPE_BYTES,
            "--scope",
        )
    }
    .map_err(|error| error.context(format!("--scope input {}", path.display())))?;
    let scope = (|| -> Result<crate::domain::search::DocumentScope> {
        let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
            CommonplaceError::InvalidInput(format!("invalid scope JSON: {error}"))
        })?;
        let scope: crate::domain::search::DocumentScope =
            serde_json::from_slice(&bytes).map_err(|error| {
                CommonplaceError::InvalidInput(format!("invalid scope JSON: {error}"))
            })?;
        scope.validate()?;
        validate(
            &value,
            &generated_schema::<crate::domain::search::DocumentScope>(),
        )?;
        Ok(scope)
    })()
    .map_err(|error| error.context(format!("--scope input {}", path.display())))?;
    Ok(scope)
}

fn parse(reader: impl Read, maximum_bytes: usize) -> Result<SchemaInput> {
    let bytes = read_bounded(reader, maximum_bytes, "schema")?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))?;
    validate(&value, &generated_schema::<SchemaInput>())?;
    // Decode the original bytes so duplicate JSON fields are not silently collapsed.
    serde_json::from_slice(&bytes)
        .map_err(|error| CommonplaceError::InvalidInput(error.to_string()))
}

pub(super) fn read_bounded(
    reader: impl Read,
    maximum_bytes: usize,
    operation: &str,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(maximum_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CommonplaceError::input_io(format!("cannot read {operation} input"), error)
        })?;
    if bytes.len() > maximum_bytes {
        return Err(CommonplaceError::LimitExceeded(format!(
            "{operation} input exceeds the {maximum_bytes}-byte limit"
        )));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn read_failures_keep_input_or_runtime_category_and_context() {
        struct Failed(std::io::ErrorKind);
        impl std::io::Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(self.0.into())
            }
        }
        for label in [
            "schema apply",
            "--scope stdin",
            "record",
            "record --jsonl",
            "withdraw",
        ] {
            for (kind, code, exit) in [
                (std::io::ErrorKind::PermissionDenied, "invalid_input", 2),
                (std::io::ErrorKind::Other, "internal_error", 1),
            ] {
                let error = super::read_bounded(Failed(kind), 16, label).unwrap_err();
                assert_eq!(error.code(), code);
                assert_eq!(error.exit_code(), exit);
                assert!(error.to_string().contains(label));
            }
        }
    }

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
