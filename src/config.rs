use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{CommonplaceError, Result};

const FORMAT: &str = "commonplace-user-config/1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserConfig {
    format: String,
    #[serde(default, deserialize_with = "deserialize_optional_path")]
    store: Option<PathBuf>,
    #[serde(default, deserialize_with = "deserialize_optional_path")]
    model_cache: Option<PathBuf>,
}

fn deserialize_optional_path<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<PathBuf>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    PathBuf::deserialize(deserializer).map(Some)
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigStatus {
    Unavailable,
    Absent,
    Loaded,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueSource {
    CommandLine,
    Environment,
    UserConfig,
    Default,
    RuntimeDefault,
}

#[derive(Debug, Serialize)]
pub struct ConfigFileReport {
    pub path: Option<PathBuf>,
    pub status: ConfigStatus,
}

#[derive(Debug, Serialize)]
pub struct PathReport {
    pub path: PathBuf,
    pub source: ValueSource,
}

#[derive(Debug, Serialize)]
pub struct OptionalPathReport {
    pub path: Option<PathBuf>,
    pub source: ValueSource,
}

#[derive(Debug, Serialize)]
pub struct ConfigReport {
    pub config_file: ConfigFileReport,
    pub store: PathReport,
    pub model_cache: OptionalPathReport,
}

#[derive(Debug)]
pub struct ResolvedConfig {
    pub store: PathBuf,
    pub model_cache: Option<PathBuf>,
    pub report: ConfigReport,
}

pub fn resolve(
    command_store: Option<PathBuf>,
    command_model_cache: Option<PathBuf>,
) -> Result<ResolvedConfig> {
    resolve_with(
        command_store,
        command_model_cache,
        &ProcessEnvironment,
        Platform::current(),
    )
}

trait Environment {
    fn variable(&self, name: &str) -> Option<OsString>;
}

struct ProcessEnvironment;

impl Environment for ProcessEnvironment {
    fn variable(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
}

#[derive(Clone, Copy, Debug)]
enum Platform {
    Macos,
    Unix,
    Windows,
}

impl Platform {
    const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

fn resolve_with(
    command_store: Option<PathBuf>,
    command_model_cache: Option<PathBuf>,
    environment: &impl Environment,
    platform: Platform,
) -> Result<ResolvedConfig> {
    let config_path = discover_config_path(environment, platform);
    let (file_status, user) = load_user_config(config_path.as_deref())?;

    let (store, store_source) = if let Some(path) = command_store {
        (path, ValueSource::CommandLine)
    } else if let Some(path) = environment.variable("COMMONPLACE_STORE") {
        (PathBuf::from(path), ValueSource::Environment)
    } else if let Some(path) = user.as_ref().and_then(|config| config.store.clone()) {
        (path, ValueSource::UserConfig)
    } else {
        (PathBuf::from(".commonplace"), ValueSource::Default)
    };

    let (model_cache, model_source) = if let Some(path) = command_model_cache {
        (Some(path), ValueSource::CommandLine)
    } else if let Some(path) = environment.variable("COMMONPLACE_MODEL_CACHE") {
        (Some(PathBuf::from(path)), ValueSource::Environment)
    } else if let Some(path) = user.as_ref().and_then(|config| config.model_cache.clone()) {
        (Some(path), ValueSource::UserConfig)
    } else {
        (None, ValueSource::RuntimeDefault)
    };

    Ok(ResolvedConfig {
        store: store.clone(),
        model_cache: model_cache.clone(),
        report: ConfigReport {
            config_file: ConfigFileReport {
                path: config_path,
                status: file_status,
            },
            store: PathReport {
                path: store,
                source: store_source,
            },
            model_cache: OptionalPathReport {
                path: model_cache,
                source: model_source,
            },
        },
    })
}

fn discover_config_path(environment: &impl Environment, platform: Platform) -> Option<PathBuf> {
    match platform {
        Platform::Macos => environment.variable("HOME").map(PathBuf::from).map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("commonplace")
                .join("config.json")
        }),
        Platform::Unix => {
            if let Some(path) = environment
                .variable("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
            {
                return Some(path.join("commonplace").join("config.json"));
            }
            environment
                .variable("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config").join("commonplace").join("config.json"))
        }
        Platform::Windows => environment
            .variable("APPDATA")
            .map(PathBuf::from)
            .map(|app_data| app_data.join("commonplace").join("config.json")),
    }
}

fn load_user_config(path: Option<&Path>) -> Result<(ConfigStatus, Option<UserConfig>)> {
    let Some(path) = path else {
        return Ok((ConfigStatus::Unavailable, None));
    };
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((ConfigStatus::Absent, None));
        }
        Err(error) => {
            return Err(CommonplaceError::Configuration(format!(
                "cannot read user configuration {}: {error}",
                path.display()
            )));
        }
    };
    let config: UserConfig = serde_json::from_slice(&bytes).map_err(|error| {
        CommonplaceError::Configuration(format!(
            "invalid user configuration {}: {error}",
            path.display()
        ))
    })?;
    if config.format != FORMAT {
        return Err(CommonplaceError::Configuration(format!(
            "unsupported user configuration format in {}; expected {FORMAT}",
            path.display()
        )));
    }
    for (name, value) in [
        ("store", config.store.as_deref()),
        ("model_cache", config.model_cache.as_deref()),
    ] {
        if value.is_some_and(|value| !value.is_absolute()) {
            return Err(CommonplaceError::Configuration(format!(
                "user configuration {name} must be an absolute path in {}",
                path.display()
            )));
        }
    }
    Ok((ConfigStatus::Loaded, Some(config)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;

    #[derive(Default)]
    struct TestEnvironment(BTreeMap<String, OsString>);

    impl TestEnvironment {
        fn with(mut self, name: &str, value: impl Into<OsString>) -> Self {
            self.0.insert(name.into(), value.into());
            self
        }
    }

    impl Environment for TestEnvironment {
        fn variable(&self, name: &str) -> Option<OsString> {
            self.0.get(name).cloned()
        }
    }

    #[test]
    fn discovers_platform_configuration_paths() {
        let mac = TestEnvironment::default().with("HOME", "/Users/scout");
        assert_eq!(
            discover_config_path(&mac, Platform::Macos).unwrap(),
            PathBuf::from("/Users/scout/Library/Application Support/commonplace/config.json")
        );

        let unix = TestEnvironment::default()
            .with("HOME", "/home/scout")
            .with("XDG_CONFIG_HOME", "/var/config");
        assert_eq!(
            discover_config_path(&unix, Platform::Unix).unwrap(),
            PathBuf::from("/var/config/commonplace/config.json")
        );
        let fallback = TestEnvironment::default()
            .with("HOME", "/home/scout")
            .with("XDG_CONFIG_HOME", "relative");
        assert_eq!(
            discover_config_path(&fallback, Platform::Unix).unwrap(),
            PathBuf::from("/home/scout/.config/commonplace/config.json")
        );

        let windows = TestEnvironment::default().with("APPDATA", r"C:\Users\Scout\AppData\Roaming");
        assert_eq!(
            discover_config_path(&windows, Platform::Windows).unwrap(),
            PathBuf::from(r"C:\Users\Scout\AppData\Roaming")
                .join("commonplace")
                .join("config.json")
        );
    }

    #[test]
    fn resolves_precedence_and_reports_sources() {
        let directory = tempfile::tempdir().unwrap();
        let config_home = directory.path().join("config");
        let config_path = config_home.join("commonplace/config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let file_store = directory.path().join("file-store");
        let file_models = directory.path().join("file-models");
        std::fs::write(
            &config_path,
            serde_json::to_vec(&json!({
                "format": FORMAT,
                "store": file_store,
                "model_cache": file_models
            }))
            .unwrap(),
        )
        .unwrap();
        let environment = TestEnvironment::default()
            .with("XDG_CONFIG_HOME", config_home.as_os_str())
            .with("COMMONPLACE_STORE", "environment-store")
            .with("COMMONPLACE_MODEL_CACHE", "environment-models");
        let resolved = resolve_with(
            Some(PathBuf::from("command-store")),
            Some(PathBuf::from("command-models")),
            &environment,
            Platform::Unix,
        )
        .unwrap();
        assert_eq!(resolved.store, Path::new("command-store"));
        assert_eq!(
            resolved.model_cache.as_deref(),
            Some(Path::new("command-models"))
        );
        assert!(matches!(
            resolved.report.store.source,
            ValueSource::CommandLine
        ));
        assert!(matches!(
            resolved.report.model_cache.source,
            ValueSource::CommandLine
        ));

        let environment =
            TestEnvironment::default().with("XDG_CONFIG_HOME", config_home.as_os_str());
        let resolved = resolve_with(None, None, &environment, Platform::Unix).unwrap();
        assert_eq!(resolved.store, file_store);
        assert_eq!(resolved.model_cache, Some(file_models));
        assert!(matches!(
            resolved.report.store.source,
            ValueSource::UserConfig
        ));
    }

    #[test]
    fn absent_discovery_uses_existing_fallbacks() {
        let resolved =
            resolve_with(None, None, &TestEnvironment::default(), Platform::Unix).unwrap();
        assert_eq!(resolved.store, Path::new(".commonplace"));
        assert!(resolved.model_cache.is_none());
        assert!(resolved.report.config_file.path.is_none());
        assert!(matches!(
            resolved.report.config_file.status,
            ConfigStatus::Unavailable
        ));
        assert!(matches!(resolved.report.store.source, ValueSource::Default));
        assert!(matches!(
            resolved.report.model_cache.source,
            ValueSource::RuntimeDefault
        ));
    }

    #[test]
    fn user_fields_are_independently_optional() {
        let directory = tempfile::tempdir().unwrap();
        let config_home = directory.path().join("config");
        let config_path = config_home.join("commonplace/config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let environment =
            TestEnvironment::default().with("XDG_CONFIG_HOME", config_home.as_os_str());
        let configured_store = directory.path().join("store");
        std::fs::write(
            &config_path,
            serde_json::to_vec(&json!({
                "format": FORMAT,
                "store": configured_store
            }))
            .unwrap(),
        )
        .unwrap();
        let resolved = resolve_with(None, None, &environment, Platform::Unix).unwrap();
        assert_eq!(resolved.store, configured_store);
        assert!(resolved.model_cache.is_none());

        let configured_models = directory.path().join("models");
        std::fs::write(
            &config_path,
            serde_json::to_vec(&json!({
                "format": FORMAT,
                "model_cache": configured_models
            }))
            .unwrap(),
        )
        .unwrap();
        let resolved = resolve_with(None, None, &environment, Platform::Unix).unwrap();
        assert_eq!(resolved.store, Path::new(".commonplace"));
        assert_eq!(resolved.model_cache, Some(configured_models));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_configuration_path_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let config_home = directory.path().join("config");
        let config_path = config_home.join("commonplace/config.json");
        std::fs::create_dir_all(&config_path).unwrap();
        let environment =
            TestEnvironment::default().with("XDG_CONFIG_HOME", config_home.as_os_str());
        let error = resolve_with(None, None, &environment, Platform::Unix).unwrap_err();
        assert_eq!(error.code(), "configuration_error");
        assert!(error.to_string().contains("cannot read user configuration"));
    }

    #[test]
    fn rejects_invalid_user_configuration_even_with_overrides() {
        let directory = tempfile::tempdir().unwrap();
        let config_home = directory.path().join("config");
        let config_path = config_home.join("commonplace/config.json");
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        let environment =
            TestEnvironment::default().with("XDG_CONFIG_HOME", config_home.as_os_str());
        for input in [
            r#"{"format":"commonplace-user-config/1","unknown":true}"#,
            r#"{"format":"commonplace-user-config/1","store":"relative"}"#,
            r#"{"format":"commonplace-user-config/1","store":null}"#,
            r#"{"format":"commonplace-user-config/1","model_cache":null}"#,
            r#"{"format":"future"}"#,
            r#"{"format":"commonplace-user-config/1","store":"/one","store":"/two"}"#,
            "{",
        ] {
            std::fs::write(&config_path, input).unwrap();
            let error = resolve_with(
                Some(PathBuf::from("store")),
                Some(PathBuf::from("models")),
                &environment,
                Platform::Unix,
            )
            .unwrap_err();
            assert_eq!(error.code(), "configuration_error");
        }
    }
}
