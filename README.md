# Commonplace

A local, evidence-backed knowledge system.

Commonplace is a lean personal knowledge tool for batch document ingestion,
hybrid SQLite search, explicitly authored cited knowledge, and local graph
queries.

The Rust implementation begins with a working store-initialization slice:

```sh
cargo run --bin commonplace -- --store .commonplace init
```

The command creates the SQLite schema, FTS5 and sqlite-vec indexes, and the
derived Grafeo storage location. It is safe to rerun against a compatible
initialized store.

Development checks:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Run development checks locally. GitHub Actions CI is disabled to conserve
Actions usage; enabling or dispatching workflows requires explicit approval.

For packet work and build status, use the repo-local
[`commonplace-progress` skill](.github/skills/commonplace-progress/SKILL.md).
The execution plan links each packet's GitHub progress issue.

## Design documents

The standalone design specification is organized under
[`docs/specification/`](docs/specification/README.md):

- [Product design](docs/specification/product.md)
- [Software architecture](docs/specification/architecture.md)
- [Persistence design](docs/specification/persistence.md)
- [Implementation and acceptance](docs/specification/implementation.md)

The specifications now select stock Oxigraph and SPARQL SELECT. Production
replacement, including the version-2 store layout, is assigned to P5a; the
initialization behavior described above still uses Grafeo until that work lands.
