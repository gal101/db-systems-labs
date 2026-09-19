# Columnar Query Engine with Iceberg-Style Version Metadata

A single-process analytical query engine in Rust, written for a Scalable Data Management Systems course: inserted chunks become immutable columnar files on disk, table state is a chain of manifests, and queries run through a chunked Volcano operator pipeline.

## What it does

- Writes every inserted chunk as its own immutable columnar file, addressed by a random UUID and created write-once, so no data file is ever patched in place.
- Tracks each table as an append-only sequence of manifests; a manifest records the files added, the files logically deleted, and the per-column Min/Max statistics of the added files.
- Answers reads at any committed version by replaying the manifest deltas up to that version, which keeps older table states queryable because superseded files stay on disk.
- Prunes candidate files from metadata alone: equality and range predicates are tested against every file's `[min, max]` interval, and only surviving handles reach the scan.
- Executes queries through Volcano-style operators over columnar `TableChunk`s (`Vec<Vec<Value>>`): `ColumnTableScan`, `ColumnFilter`, `ColumnEqJoin` (in-memory hash join), `ColumnAggregate`.
- Applies updates and deletes copy-on-write: the affected chunk is rewritten into new files, and the replaced handles are retired in the next manifest.

## Architecture

**Storage (`src/storage/`)** — a hand-rolled little-endian columnar file format.

- A file is: 8 magic bytes (`53 44 4d 53 19 03 4a 53`), `u64` row count, `u64` column count, one `(u64 TypeID, u64 start offset)` pair per column, then the column payloads back to back. `Int`/`UInt` take 4 bytes, `RowID` 8, `Varchar` a `u64` length prefix plus UTF-8 bytes (`src/storage/data.rs`).
- `DataFile::to_bytes` computes each column's start offset in a single pass and serialises header plus data; `DataFile::parse` reads all columns, while `parse_columns(Columns::Selection(..))` seeks straight to the offsets of the requested columns and returns empty vectors for the rest.
- `FileBasedStorage` maps a `FileHandle` UUID to `<base_path>/xx/yy/<28 hex chars>.bin`, creating directories on demand; the default base path is `./target/sdms`. Writes use `create_new(true)`, so a name collision is an error rather than an overwrite (`src/storage/file_storage.rs`).
- Round-tripping the committed fixtures through `parse` and `to_bytes` reproduces the original byte stream exactly; a 2-column, 3-row fixture is 80 bytes.

**Metadata (`src/iceberg/`)** — the versioned catalogue.

- `Catalog` is a `Vec<TableMetadata>` indexed by table id, so `add_table` returns the table's index. `TableMetadata` holds the schema, the current `Version`, and `manifests: Vec<Manifest>`.
- `Manifest` is exactly `added: Vec<FileHandle>`, `deleted: Vec<FileHandle>`, `stats: Vec<FileStats>`, with `stats[i]` describing `added[i]`.
- `Version` is a `u64` counter incremented by `TableMetadata::add_version` (a new manifest is only appended by commit).
- `snapshot(Some(v))` yields `manifests[0..v]`; `files(version)` replays those manifests, adding every `added` handle and removing every `deleted` handle; `contains`/`contains_range` intersect predicates against the per-file statistics and then retain only the handles that are live at that version.
- `MinMax::contains` / `MinMax::contains_range` test interval overlap (`min <= range.end && range.start <= max`), conjunctively across predicates; `MinMax::new` panics when `min > max`, and `Value`'s `Ord` implementation compares like types and panics on mixed types (`src/iceberg/stats.rs`, `src/value_cmp.rs`).

**Execution (`src/engine/`)** — transaction handling and query operators.

- `SdmsIcebergEngine` owns the catalog, the storage handle, one in-flight `manifest`, the current `table_id`, and a `changed_files` set.
- Write path: `start_table_modification(table_id)` opens a transaction, `insert`/`update`/`delete`/`delete_chunks` mutate files and the in-flight manifest, `commit` appends the manifest as a new version only if something changed. Any second modification of a file already touched in the same transaction is rejected.
- Read path: the caller resolves handles with `files` / `contains` / `contains_range`, wires an operator chain by hand, then drives `open()` / `next()` until `None`. `benches/basic_bench.rs` shows that prune-then-scan pattern.
- `ColumnTableScan` emits one chunk per data file; `ColumnFilter` keeps rows where every filtered column value lies inside its inclusive `[min, max]` bound; `ColumnEqJoin` fully consumes its left child in `open()`, builds a `HashMap<Value, usize>` from left join-key values to row indexes, then streams the right child and emits matched pairs; `ColumnAggregate` computes a partial result per chunk and merges the partials into a single output row.
- `src/engine/optimizer.rs` declares `QueryPlan` (`TableScan`, `Filter`, `Aggregate`, `Join`) and `Optimizer::re_cluster`, whose body is `todo!()`. Nothing converts a `QueryPlan` into an operator tree; the plan enum is only used as a workload description.

## Provenance and scope

This is the **implementation extract** of a university lab assignment. What you see here
is the part I wrote — the data structures and algorithms described above. It is published
so the design can be read and reviewed.

The surrounding framework is **not** included and is **not** redistributed. The module
skeleton, build files, the course's test harness, its fixtures and the CI configuration
all belong to the course, not to me. Two consequences follow, and both are deliberate:

- **This will not compile or run on its own.** The modules below implement traits, types
  and interfaces that are declared in the omitted files.
- **There are no benchmark or test numbers here**, because the harness that produces them
  is not mine to publish.

Read it as a code sample, not as a runnable project.

## Key design decisions

- **One file per inserted chunk.** The caller's chunk size sets the unit of pruning, of copy-on-write rewriting and of one scan chunk. Small chunks give finer pruning but more files, more manifest entries and less write efficiency; there is no compaction or re-clustering to repair a bad choice, since `re_cluster` is unimplemented.
- **Min/Max statistics only, and only per file.** They are cheap to compute on write and to compare on read, but a single outlier widens a file's interval so the whole file is read. There is no histogram, dictionary or per-page statistic, so pruning granularity is the file, not a page.
- **Pruning is an API the caller invokes, not a planner decision.** `files` / `contains` / `contains_range` are pure metadata functions returning handles; nothing rewrites a query automatically, so an unscanned prune is always available and equally easy to forget.
- **Deletes are logical.** Updates and deletes rewrite whole files and only retire handles in the next manifest; nothing calls `remove_file`, which is exactly what keeps earlier versions readable, at the cost of unbounded space growth with no vacuum.
- **Versioning is single-writer.** `snapshot(Some(v))` slices the first `v` manifests and silently clamps out-of-range versions to the current state, `Version` wraps on overflow, the engine refuses to open a second modification while one is in flight, and `DatabaseError::OptimisticFail` is declared but never returned. Version-addressed reads are therefore safe for one writer, but there is no isolation from concurrent writers, no conflict detection, and uncommitted changes are invisible even to the transaction that made them.

## Where to look first

- `src/iceberg/table_metadata.rs` — `snapshot`, `files` and the two pruning entry points. Note that a version is a manifest count, that `Version::new(0)` means the empty table, and that pruning keeps only handles still live at the requested version.
- `src/engine/db_engine.rs` — `update` and `delete` are the copy-on-write path: read one file, edit in memory, write a new file, add its fresh statistics, mark the old handle deleted. `changed_files` is what makes a repeated modification of one file inside a transaction fail rather than silently lose data.
- `src/engine/operators.rs` — `ColumnEqJoin::open` materialises the entire left child and its hash map keeps only the last row per key, so duplicate build keys collapse and the left input must fit in memory; `ColumnFilter` treats every bound as inclusive and tolerates the empty vectors that projection leaves behind.
- `src/storage/data.rs` — `to_bytes` and `parse_column` show the header/offset scheme end to end; `parse_columns` is the seek-based column reader that the scan operator does not currently use.
- `benches/basic_bench.rs` — the only executable example of a full query plan, and the place where the intended metadata-first access pattern is visible.

### What is original here

The course distributed a skeleton and a grading runner. Files whose first line is a note that the runner replaces them are scaffolding and are not the author's work: `src/lib.rs` (the `Value`, `Record`, `TypeID`, `RowID`, `DatabaseError` and `Operator` definitions and the `TableChunk`/`Schema` aliases), `src/iceberg/mod.rs` (`Catalog`, `TableMetadata`, `Manifest`, `FileStats`, `ColumnStats`, `MinMax` struct declarations), `src/iceberg/catalog.rs`, `src/iceberg/version.rs`, `src/engine/mod.rs`, `src/storage/mod.rs`, `src/storage/file_storage.rs`, `Cargo.toml`, `rust-toolchain.toml`, the `*_tests_*.rs` harnesses and the two `benches/`.

Everything without that note is what the runner executes as submitted work: `src/value_cmp.rs`, `src/storage/data.rs`, `src/iceberg/manifest.rs`, `src/iceberg/stats.rs`, `src/iceberg/table_metadata.rs`, `src/engine/db_engine.rs`, `src/engine/operators.rs` and the `todo!()` body of `src/engine/optimizer.rs`. Inside those files the signatures and doc comments that read like a specification come from the course skeleton; the function bodies implementing the format, the statistics, the pruning queries, the version replay and the four operators are the author's. The unimplemented advanced test modules and the unimplemented query benchmark are the author's too — that is, deliberately left empty.

### Known limitations

Deliberate scope boundaries, stated precisely so nothing here is over-read:

- **Pruning is file-granular.** Statistics are per-file, per-column Min/Max, and pruning is metadata-only — but it is invoked by the caller; there is no automatic planner (`re_cluster` is `todo!()`), and there is no page, row-group or block concept or statistic anywhere in the code.
- **Column selection is not a storage-level pushdown in the executed path.** `DataFile::parse_columns` can read selected columns at I/O level, but `ColumnTableScan` calls `DataFile::parse` and then blanks unprojected columns in memory, so the scan reduces downstream work, not bytes read.
- **"Micro-partition" here means one file per chunk.** Each file is created by an insert or rewrite call; there is no partitioning key, no clustering and no automatic sizing.
- **Time travel is version-addressed metadata replay, not an isolation level.** It works because files are immutable and never unlinked, so earlier states stay readable. Writing is single-writer with no conflict detection (`OptimisticFail` is declared but unused) and no read-your-own-uncommitted-writes.
- **Nothing here is benchmarked.** `benches/query_bench.rs` is `unimplemented!()` and no measurement is committed.
