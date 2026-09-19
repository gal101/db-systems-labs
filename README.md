# Database and Systems Lab Implementations

Implementation extracts from six university lab assignments in database systems and
computer systems, written in 2025–2026. Each directory is the part of the assignment I
wrote; the surrounding course framework is not included.

| Directory | What it implements | Language |
|---|---|---|
| [`radix-join/`](radix-join/) | Multi-threaded cache-conscious radix equi-join: three-phase lock-free partitioning, per-bucket hash join with atomic work-stealing | C++20 |
| [`rdma-shuffle/`](rdma-shuffle/) | Distributed shuffle operator over one-sided RDMA verbs, zoned DMA memory layout, `fetch_add`-based barrier | C++20 |
| [`buffer-pool/`](buffer-pool/) | Disk manager and buffer pool: page layout, pin/unpin semantics, LRU and CLOCK replacement | Rust |
| [`index-and-operators/`](index-and-operators/) | B+tree index, k-way merge, and Volcano-model operators (sort, equi-join, table scan) | Rust |
| [`distributed-operators/`](distributed-operators/) | Exchange and distribution operators, distributed join, three bitmap index variants, lock table and multi-granularity locking | Rust |
| [`columnar-engine/`](columnar-engine/) | Columnar storage over immutable data files, per-column Min/Max statistics, manifest-based table versioning | Rust |

## Provenance and scope

These are **implementation extracts**, not complete projects. For each one, the
surrounding framework — module skeleton, build files, test harness, fixtures, CI
configuration — belongs to the university course and is **not** redistributed here.

Two consequences follow, and both are deliberate:

- **None of these directories compile or run on their own.** The modules here implement
  traits, types and interfaces that are declared in the omitted files.
- **There are no benchmark or test numbers committed here**, because the harness that
  produces them is not mine to publish.

Each directory's README documents what the code does, how it is put together, the design
decisions behind it, and an explicit note on which parts were provided versus written.

Read these as code samples, not as runnable projects.
