# Local release packaging

This is a manual **macOS arm64 developer-host** procedure, not release
publication or independent clean-machine certification. P10 [#22](https://github.com/markgar/commonplace/issues/22)
owns live artifacts, results, blockers and review status. macOS x64 is not a
supported target. Windows x64/arm64 and Linux x64/arm64 require separate native
execution and clean-target evidence; the current Windows directory-publication
implementation also needs correction and verification.

## Build and extract

Use the checked-in Rust toolchain, Xcode C/C++ tools and Python 3.11+ on the
build host. Python is used only to assemble the package, never by the application.
Keep Cargo outputs in this worktree's absolute `target` directory. No other
session's mutable binaries or build outputs may be reused.

Start from a clean committed tree and an already prepared, verified model cache:

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
export CARGO_TARGET_DIR="$PWD/target"
export COMMONPLACE_MODEL_CACHE=/absolute/path/to/prepared/pinned-models
python3 scripts/package-macos.py
```

The helper builds the real application with `--release --locked --offline -j1`
under network denial. It checks the pinned native ONNX Runtime cache before
building. Missing Rust/native/model caches are blockers, not permission to fetch
from alternate sources. The build environment deliberately excludes native
library/provider overrides and uses the repository's default selected stack.
The helper refuses dirty source, other architectures, output replacement,
invalid model hashes and non-system Mach-O dependencies or search paths. An
unexpected native dependency needs investigation, not an automatic fallback or
load-path rewrite.

Output is `target/dist/commonplace-<version>-<commit>-macos-arm64.tar.gz`, a
sidecar SHA-256, and a build log. The archive contains the copied executable,
exactly the ten pinned model files, `Cargo.lock`, model identities, native/build
provenance, usage instructions and `SHA256SUMS`. Models are read-only copies.
The existing binary signature is verified; the helper does not sign or notarize.
No model acquisition occurs. Native cached-library hashes identify the actual
local inputs; they do not retroactively prove the original download.

Extract into a fresh directory outside the source and build trees. For example,
with `archive` set to the exact helper output:

```sh
archive=/absolute/path/to/the/exact/archive.tar.gz
(cd "$(dirname "$archive")" && shasum -a 256 -c "$(basename "$archive").sha256")
work=$(mktemp -d /tmp/commonplace-package.XXXXXX)
tar -xzf "$archive" -C "$work"
bundle="$work/$(basename "$archive" .tar.gz)"
(cd "$bundle" && shasum -a 256 -c SHA256SUMS)
codesign --verify --strict "$bundle/commonplace"
file "$bundle/commonplace"
otool -L "$bundle/commonplace"
mkdir "$work/home" "$work/tmp"
```

Record the resolved `work`/`bundle` paths, source commit/tree, archive and binary
hashes, native link/load evidence, model hashes, host OS/CPU/SDK and toolchain.
Keep extraction and TMPDIR on the same filesystem: the existing search test
uses read-only hardlinks to create two model-cache variants. These are not links
to another session's executable or original model cache. Never corrupt or
modify a linked model; use an independent disposable copy for corruption checks.

## Stable local installation and persistent store

`USAGE.txt` inside the archive is a self-contained install guide. The following
uses only standard macOS utilities, requires no administrator access, and
refuses to replace an existing installation. Start after extraction and checksum
verification above:

```sh
release_dir="$HOME/.local/share/commonplace/releases/$(basename "$bundle")"
bin="$HOME/.local/bin/commonplace"
test ! -e "$release_dir" && test ! -e "$bin" || exit 1
mkdir -p "$(dirname "$release_dir")" "$(dirname "$bin")"
cp -R "$bundle" "$release_dir"
install -m 755 "$release_dir/commonplace" "$bin"
(cd "$release_dir" && shasum -a 256 -c SHA256SUMS)
cmp "$release_dir/commonplace" "$bin"
```

The executable is now the stable path `$HOME/.local/bin/commonplace`, not a
worktree, `target`, temporary-directory or Copilot-session binary. The versioned
release directory retains the immutable models and provenance. For each
terminal or local agent process, provide the existing environment variables:

```sh
export PATH="$HOME/.local/bin:$PATH"
export COMMONPLACE_MODEL_CACHE="$release_dir/pinned-models"
store="$HOME/.local/share/commonplace/stores/personal"
commonplace --store "$store" init
commonplace --store "$store" ingest /absolute/path/to/notes
commonplace --store "$store" search "release planning"
```

Alternatively, deliberately create the optional macOS user configuration once:

```sh
config="$HOME/Library/Application Support/commonplace/config.json"
store="$HOME/.local/share/commonplace/stores/personal"
mkdir -p "$(dirname "$config")"
tmp="$config.tmp"
cat > "$tmp" <<EOF
{
  "format": "commonplace-user-config/1",
  "store": "$store",
  "model_cache": "$release_dir/pinned-models"
}
EOF
mv "$tmp" "$config"
commonplace config show
commonplace init
```

This file is user-owned and distinct from the selected store's backend-managed
`config.json`. The package helper and installation commands do not create or
replace it. There is no `--config`, profile, credential, migration, or config
writer. Use explicit `--store`/`--model-cache` or
`COMMONPLACE_STORE`/`COMMONPLACE_MODEL_CACHE` overrides for another invocation.

For a later process, use the exact installed absolute model path rather than
assuming that its shell still has `release_dir`. Agents may instead invoke the
absolute executable and set `COMMONPLACE_MODEL_CACHE` explicitly in their
process environment; no shell profile is required.
The user store is a separate persistent directory, never part of a release
archive or replacement operation. Keep the archive/checksum and do not silently
overwrite an existing installation or mutate/migrate an incompatible store.
The local validation installs into a disposable isolated HOME using these same
paths; it does not install into the developer's real HOME or personal store.

## Run the extracted binary, not Cargo's binary

Run the README format, Clippy and ordinary test checks locally with locked,
offline Cargo and `-j1` for builds. Run tests with `--test-threads=1` and keep
builds and real-model invocations sequential to reduce thermal load. Then compile
the existing integration-test controllers without running them:

```sh
sandbox-exec -p '(version 1)(allow default)(deny network*)' \
  cargo test --locked --offline -j1 --no-run --message-format=json \
  > "$work/test-build.jsonl"
```

Cargo's JSON `compiler-artifact` records with `profile.test=true` and the
requested `target.name` supply the exact controller `executable` paths. Do not
guess a hashed filename or substitute a stale executable. Controllers are test
scaffolding; they can inspect SQLite and use checked-in fixtures. The application
itself requires none of those build tools.

For the `real_models` controller's exact executable path:

```sh
controller=/absolute/path/from/the/current/compiler-artifact/record
(cd "$work" && env -i PATH=/usr/bin:/bin HOME="$work/home" TMPDIR="$work/tmp" \
  COMMONPLACE_MODEL_CACHE="$bundle/pinned-models" \
  COMMONPLACE_TEST_BINARY="$bundle/commonplace" \
  /usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)' \
  "$controller" --ignored --test-threads=1 --nocapture) \
  > "$work/real-models.log" 2>&1
```

All three explicit cases must pass, including unchanged 4/4 top-five retrieval
expectations. The existing file lifecycle now also streams a stable non-file
source, searches before authoring, withdraws a cited fact before removal, and
finishes with reingest/reopen search and graph parity. The separate stream test
keeps JSONL partial-failure coverage.

`tests/common/mod.rs::Store::command` uses `COMMONPLACE_TEST_BINARY` only in
tests. A supplied selector must be absolute, regular and executable; errors
never fall back. Without the selector ordinary tests retain Cargo's default.
Each controller prints an unconditional `COMMONPLACE_TEST_BINARY_RECEIPT` with
selection mode, canonical executable path and SHA-256, once on first Store use.
Public CLI stdout/stderr contracts are unchanged.

**A passing test exit is insufficient package evidence.** Gate the captured
receipt against the extracted manifest and actual path. For example:

```sh
python3 - "$work/real-models.log" "$bundle" <<'PY'
import hashlib, json, pathlib, sys
log, bundle = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
binary = (bundle / "commonplace").resolve()
expected = next(line.split()[0] for line in (bundle / "SHA256SUMS").read_text().splitlines()
                if line.split(maxsplit=1)[1] == "commonplace")
marker = "COMMONPLACE_TEST_BINARY_RECEIPT "
receipts = [json.loads(line.split(marker, 1)[1]) for line in log.read_text().splitlines()
            if marker in line]
assert receipts, "missing in-process binary receipt"
assert all(r == {"mode": "external", "path": str(binary), "sha256": expected}
           for r in receipts), receipts
with binary.open("rb") as source:
    assert hashlib.file_digest(source, "sha256").hexdigest() == expected
PY
```

Repeat this receipt gate for every controller invocation called packaged CLI
evidence. Verify all package hashes again after execution. The receipt gate is
intentionally separate from the default source-suite behavior: omission of both
a selector and a hypothetical "required" flag must not become apparent success.

Negative controls use the exact `init` test controller and
`--exact binary_preserves_init_contract_and_existing_store --nocapture`:

- Omit the selector: its receipt says `cargo`, so the package receipt gate must
  reject it even if the source test passes.
- Set the selector to a nonexistent absolute path: the controller must fail
  explicitly without executing Cargo's binary.
- A relative selector or a non-executable regular file must also fail.

Use fresh HOME, TMPDIR and cwd, and pass every required environment variable
**after** `env -i`. Runtime invocations use only the extracted model cache, never
the original shared cache. Also run the binary directly with an absent cache for
non-inference init/schema/graph commands, then require explicit
`model_unavailable` from nonempty ingestion. Test corrupt model failure only
against an independent disposable cache copy. No cold acquisition is claimed.

This procedure leaves developer tools installed on the host. Sanitized PATH,
HOME/cwd, copied caches, offline execution and inspected system-only linkage
are **local relocation evidence**, not an independent clean-machine result.
Runtime access to fixture *inputs* by a controller is not a hidden inference
dependency. Signing, notarization, redistribution notices/model terms and older
macOS compatibility require separate release decisions/evidence.

## Section 6 evidence map

The following maps owning [acceptance checks](specification/implementation.md#6-release-gating-checks)
to existing tests, not a replacement contract or a claim that every target
passes. Record exact commands/results and revision in #22.

**S** = source-built tests with real SQLite/Oxigraph and deterministic inference
where appropriate. **P** = selected extracted binary's public CLI calls, with
receipt gate. **R** = real-model controller/library assertions. **M** = manual
package inspection. Mixed tests must retain these distinctions.

| Gates | Existing evidence location | Category |
| --- | --- | --- |
| One model load, bounded batches, independent document failures | `tests/ingest.rs`: `complete_pipeline_retains_exact_revisions_and_current_indexes`, `failures_are_independent_and_search_publication_rolls_back`, `configured_source_passage_and_request_bounds_are_enforced` | S |
| Malformed JSONL/repeated keys preserve prior success | `public_stream_failures_and_terminal_caps_preserve_prior_successes`; `real_model_stream_ingest_get_offline` | S; P/R |
| Direct/directory identity, FTS/vector correlation and row IDs | `complete_pipeline_retains_exact_revisions_and_current_indexes`; `real_model_ingest_get_offline` | S; P/R |
| Exact UTF-8 citations and metadata revisions | `every_metadata_field_revises_and_empty_replacement_clears_only_current_indexes`; all three real-model tests | S; P/R |
| Scan absence never deletes | `scan_absence_never_deletes_and_disappeared_inputs_fail_individually`; `file_keys_and_directory_absence_do_not_remove_or_touch_original_files` | S/P |
| Both retrieval paths, deterministic fusion/deduplication, bounded reranking | `tests/search.rs`: `both_real_candidate_paths_contribute_deduplicate_and_rerank_with_bounds`, `fusion_ties_and_duplicate_elimination_are_deterministic` | S |
| Complete hydrated evidence, filters/result limits | `exact_hydration_and_one_snapshot_survive_current_revision_replacement`, `filters_precede_both_cutoffs_and_preserve_nanoseconds_offsets_and_nulls`, `candidate_cutoff_ties_and_result_edges_have_exact_truncation`; `real_model_search_offline` | S; P/R |
| Explicit missing/incompatible model/index errors | `missing_corrupt_or_malformed_required_models_fail_instead_of_lexical_only`, `missing_extra_and_wrong_dimension_indexes_fail_even_when_empty_or_filtered_out`, `incompatible_embedding_identity_fails_before_inference`; package absent/corrupt-cache checks | S; P |
| Additive atomic schema, endpoint-only version increment | `tests/schema.rs`: `apply_reopen_noop_and_endpoint_only_versioning`, `invalid_requests_are_atomic_and_descriptions_are_immutable` | P plus S database assertions |
| Multiple entity types; unique identifiers/ambiguous names; atomic alias/identifier correction | `tests/record.rs`: `multiple_types_exact_old_new_evidence_canonical_get_query_and_reopen`, `invalid_late_items_ambiguous_names_identifiers_and_refs_are_atomic`, `bare_empty_and_metadata_only_do_not_publish_or_repair_graph` | S/P |
| Quote/offset rejection; duplicate successful knowledge IDs | `malformed_support_rolls_back_all_changes`, `relationships_literals_resulting_types_exact_evidence_and_fact_only_publication`; real-model lifecycle | S/P |
| Validation/build rollback; activation failure; retained COMMIT failure/restoration | `src/graph/runtime_tests.rs`: `coordinated_build_activation_and_real_commit_failures_restore_old_state`, `activation_failures_restore_exact_previous_directory`, `commit_restoration_failure_is_explicit_and_fail_closed`, removal/withdrawal publication failure cases | S; not packaged fault injection |
| Atomic withdrawal with retained history | `tests/withdraw.rs`: `invalid_late_ids_and_already_withdrawn_reject_before_any_mutation`, `complete_batch_checks_both_endpoint_directions_and_allowed_alternatives`, `retained_subtypes_literals_evidence_provenance_and_active_graph_survive_reopen`; real-model lifecycle | S/P |
| All active/no withdrawn RDF, used vocabulary only, empty version metadata | `tests/graph.rs`: `init_schema_query_rebuild_reopen_and_unused_vocabulary`; withdrawal tests; real-model lifecycle | S/P |
| Ingest leaves graph unchanged; exact knowledge/passage/revision/document traversal; distinct equal-fact IRIs | `real_model_ingest_get_offline`; `membership_identity_evidence_bytes_and_complete_mapping_survive_rebuild`; fact tests in `tests/record.rs` | S/P/R |
| Version matching, invalid/missing/corrupt graphs fail closed, explicit rebuild | `unavailable_graphs_fail_closed_and_explicit_rebuild_recovers_known_scratch_only`, `old_unknown_and_relabeled_layouts_are_never_mutated`; `actual_process_exit_windows_fail_closed_and_rebuild_only_committed_state` (crash windows, not Windows OS) | S/P |
| Reader leases exclude publication | `two_reader_processes_exclude_publication_and_termination_releases_leases`, record/remove/withdraw lease cases; public query termination case | Mixed: holders and some contenders are controller/library, not P |
| Native read-only mutation rejection, SELECT-only parser, remote SERVICE disabled | `native_read_only_rejects_mutation`, `native_terms_unbound_columns_local_service_and_query_forms` | S; P for public query paths |
| Native lazy `limit+1`, no injected LIMIT, RDF-term JSON fidelity | `collector_consumes_only_limit_plus_one_native_solutions`, `bounded_rows_preserve_native_order_limit_offset_distinct_and_aggregates`, `native_terms_unbound_columns_local_service_and_query_forms` | S/P |
| Cooperative cancellation during execution/iteration, prompt timer teardown, process termination | `cancellation_covers_execution_and_iteration_without_partial_success`, `completed_and_expired_timers_join_promptly`, `sigint_terminates_query_and_releases_publication_lease` | S; P for CLI query termination |
| Foreign keys/STRICT; incompatible stores; one timestamp per operation | `tests/store.rs`: `sqlite_constraints_and_rollback_are_active_in_write_sessions`, `incompatible_or_incomplete_stores_fail_before_mutation`; init/record/withdraw tests | S/P |
| Process writer lock | `tests/schema.rs`: `writer_contention_is_bounded_and_termination_releases_lock` | P contender, source-controller lock holder; other platforms unverified |
| Postcommit failures retain committed receipts and do-not-retry guidance | `all_post_commit_cleanup_errors_preserve_committed_receipt_and_current`; `public_response_delivery_failure_reports_committed_withdrawal_without_retry` | S; P response-delivery test |
| No weights for non-inference; no remote source-text inference | package absent-cache checks, model lazy-loading tests, network-denied public real-model suite | S/P |
| Native extensions/Oxigraph in actual package | helper provenance/linkage and extracted full public lifecycle | P/M locally; independent clean-target gate UNVERIFIED |

Lock details: `common::LockHolder`, `graph_reader`, and
`native_writer_contender` spawn the source-built controller. The two-reader
graph test also calls library rebuild directly. The schema contention test
uses the extracted CLI as contender; the SIGINT test runs a packaged query.
Do not report these collectively as two packaged CLI processes.

| Supported target | Independent clean-target release status |
| --- | --- |
| macOS arm64 | UNVERIFIED; developer-host package evidence is separate |
| Windows x64 | BLOCKED: directory-publication implementation/verification and native package execution |
| Windows arm64 | BLOCKED: directory-publication implementation/verification and native package execution |
| Linux x64 | UNVERIFIED; no local execution authorized |
| Linux arm64 | UNVERIFIED; no local execution authorized |

## Windows handoff: discovery before implementation

No Windows host is assumed available. Do not install tools, obtain credentials,
elevate permissions, enable remote access or download dependencies by implication.
When the user provides a host, begin with read-only PowerShell discovery:

```powershell
[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
[System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture
[System.Environment]::OSVersion.VersionString
$PSVersionTable
Get-Command rustc,cargo,git,cl,clang -ErrorAction SilentlyContinue
```

For tools actually present, collect `rustc -Vv`, `cargo -V`, `git --version`,
and compiler/SDK/libclang locations. Distinguish native architecture from an
emulated shell/toolchain. The pinned RocksDB build uses C/C++20 and bindgen:
MSVC/SDK and compatible libclang are build prerequisites, not runtime tools.
The pinned ort-sys distribution catalog has x64 and arm64 MSVC entries, both
with DirectML system-link prerequisites; catalog entries are not successful
Windows build/runtime evidence. Discover existing Cargo/native/model caches and
the chosen scratch filesystem before planning execution.

After host availability and separate execution approval, use the **exact source
commit in the accepted artifact's provenance / #22 handoff**, confirm
`git rev-parse HEAD`, and verify any transferred files with
`Get-FileHash -Algorithm SHA256`. No macOS binary can stand in for a Windows one.
In the dedicated Windows checkout, build only into its own absolute target:

```powershell
$env:CARGO_TARGET_DIR = Join-Path (Get-Location).Path 'target'
cargo build --release --locked --offline -j1 --bin commonplace
```

This is a future approved native-host command, not executed macOS evidence.
Cargo offline does not constrain native build-script downloads. Missing native
caches must stop the attempt; agree a host-specific network-denial mechanism
before execution, without changing OS policy or assuming administrator rights.
Do not set `LIBCLANG_PATH` or toolchain overrides until the actual installation
has been identified. If prerequisites are absent, return the precise missing
component; do not install or procure it automatically.

First reproduce `init` using that exact Windows binary and a fresh disposable
store outside its source/build tree. Capture exit status and JSON stdout/stderr;
do not run against any existing personal store. Then agree the smallest native
publication experiment before proposing production code. Required observations
span init (including outer staging rename), candidate activation, rollback and
restoration, failed-candidate removal, rebuild and postcommit cleanup.

`src/graph/runtime.rs::sync_directory` currently uses
`File::open(directory).sync_all()`. Relevant first-party API documentation:

- [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew):
  directory handles require `FILE_FLAG_BACKUP_SEMANTICS`.
- [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers):
  requires `GENERIC_WRITE`; volume-wide flushing requires administrator access.
- [MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw):
  same-drive directory moves are supported, replacement of an existing
  directory is not, and the explicit `WRITE_THROUGH` flush guarantee discusses
  copy/delete moves.

Those statements alone do not establish a complete unprivileged directory
create/rename/delete durability path. A backup-semantics flag alone, ignored
flush errors, a no-op, or privileged volume flush is not the proposed fix.
Investigate access/share flags, native error codes, same-volume rename/flush
semantics, delete-pending cleanup, and handle closure. Application publication
leases must still exclude readers *before* renames; OS sharing failures are
explicit errors, not a substitute lock. Preserve retained COMMIT failure
handling, fail-closed versions, source/knowledge safety and postcommit receipts.

Return OS/CPU/process architecture, filesystem, privileges used, exact revision,
native dependency and binary hashes, commands, exit/JSON/native-error evidence,
lock/restore/cleanup observations and unresolved questions. Successful API calls
are not arbitrary power-loss proof. Any production Windows API/dependency
change needs a concrete rationale and follow-up coordinator approval.
