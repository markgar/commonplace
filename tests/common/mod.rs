#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub struct Store {
    pub directory: tempfile::TempDir,
    pub root: PathBuf,
}

impl Store {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("store");
        let store = Self { directory, root };
        store.success(&["init"]);
        store
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_commonplace"));
        command.arg("--store").arg(&self.root);
        command
    }

    pub fn run(&self, arguments: &[&str]) -> Output {
        self.command().args(arguments).output().expect("run CLI")
    }

    pub fn success(&self, arguments: &[&str]) -> Value {
        let output = self.run(arguments);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice(&output.stdout).expect("JSON result")
    }

    pub fn input(&self, value: &Value) -> PathBuf {
        let path = self.directory.path().join("schema.json");
        std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }

    pub fn apply(&self, value: &Value, check: bool) -> Value {
        let path = self.input(value);
        let mut args = vec!["schema", "apply", path.to_str().unwrap(), "--json"];
        if check {
            args.push("--check");
        }
        self.success(&args)
    }

    pub fn failure(&self, arguments: &[&str], code: &str, exit: i32) -> Value {
        let output = self.run(arguments);
        assert_eq!(output.status.code(), Some(exit), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stderr).expect("JSON error");
        assert_eq!(value["error"]["code"], code, "{value}");
        assert_eq!(value["status"], "failed");
        value
    }

    pub fn database(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.join("commonplace.sqlite3")).unwrap()
    }

    pub fn files(&self) -> BTreeMap<PathBuf, Vec<u8>> {
        fn collect(path: &std::path::Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in std::fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    collect(&path, files);
                } else if matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("commonplace.sqlite3-wal" | "commonplace.sqlite3-shm")
                ) {
                    // Read-only WAL snapshots may create SQLite's own bookkeeping.
                    // Never ignore application-owned files or logical state changes.
                    continue;
                } else {
                    files.insert(path.clone(), std::fs::read(path).unwrap());
                }
            }
        }
        let mut files = BTreeMap::new();
        collect(&self.root, &mut files);
        files
    }

    pub fn assert_files(&self, before: &BTreeMap<PathBuf, Vec<u8>>) {
        let after = self.files();
        let changed: std::collections::BTreeSet<_> = before
            .keys()
            .chain(after.keys())
            .filter(|path| before.get(*path) != after.get(*path))
            .collect();
        assert!(changed.is_empty(), "changed files: {changed:?}");
    }

    pub fn vocabulary() -> Value {
        json!({
            "entity_types": [{"name":"person", "description":"A person"}, {"name":"company"}],
            "identifier_schemes": [{"name":"email", "description":"Email address"}],
            "predicates": [
                {"name":"works_at", "object_kind":"entity", "subject_types":["person"], "object_types":["company"]},
                {"name":"age", "object_kind":"integer", "subject_types":["person"]},
                {"name":"active", "object_kind":"boolean", "subject_types":["person"]},
                {"name":"born", "object_kind":"timestamp", "subject_types":["person"]},
                {"name":"note", "object_kind":"string", "subject_types":["person"]}
            ]
        })
    }
}

pub struct LockHolder(Child);

impl LockHolder {
    pub fn start(store: &Store) -> Self {
        let ready = store.directory.path().join("lock-ready");
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "lock_holder", "--nocapture"])
            .env("COMMONPLACE_TEST_STORE", &store.root)
            .env("COMMONPLACE_TEST_READY", &ready)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut holder = Self(child);
        let start = Instant::now();
        while !ready.exists() {
            assert!(holder.0.try_wait().unwrap().is_none(), "lock holder exited");
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "lock holder timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        holder
    }
}

impl Drop for LockHolder {
    fn drop(&mut self) {
        self.0.kill().expect("terminate lock holder");
        self.0.wait().expect("reap lock holder");
    }
}

pub fn hold_writer_lock() {
    let root = PathBuf::from(std::env::var_os("COMMONPLACE_TEST_STORE").expect("helper store"));
    let mut session =
        commonplace::storage::database::SqliteDatabase::write(&root, Duration::ZERO).unwrap();
    let _transaction = session.transaction().unwrap();
    std::fs::write(
        std::env::var_os("COMMONPLACE_TEST_READY").unwrap(),
        b"ready",
    )
    .unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
