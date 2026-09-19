# Distributed Operators, Bitmap Indexing, and Locking

A Rust crate built for the Scalable Data Management Systems course at TU Darmstadt covering three
database-engine topics side by side: distributed query operators, bitmap indexes, and locking.

## What it does

- Provides a Volcano-style `Exchange` operator that shuffles `Record`s between cluster peers per a
  pluggable distribution function, behind the usual `open` / `next` / `close` iterator interface.
- Composes four distributed join plans from small primitives: symmetric repartitioning, asymmetric
  repartitioning, replication (broadcast) join, and semi-join reduction.
- Implements three interchangeable bitmap indexes behind one trait,
  `range_lookup(start, end) -> IndexIterator`: naive per-value, cumulative range-encoded, base-N
  decomposed.
- Implements a row-level lock table with shared and exclusive requests, per-object FIFO wait queues
  and in-place read-to-write upgrades, deliberately without deadlock prevention (wait-die,
  wound-wait and no-wait are excluded by the spec).
- Implements multi-granularity locking over a named resource tree with intention modes (`IS`, `IX`)
  on ancestors and `S` / `X` on the target node, acquired all-or-nothing along a path.

## Architecture

Data model (`src/lib.rs`): `Record` is a `Vec<Value>`; `Value` is `Int(i32)`, `UInt(usize)` or
`RowID`. Operators implement `Operator` (`open`, `close`) on top of `Iterator`, boxed as
`DynOperator = Box<dyn Operator<Item = Record> + Send>`; `test_util::TableScan` is the leaf scan.

### Distributed execution (`src/dist_operator/`)

**Exchange** (`exchange/exchange_impl.rs`; struct in `exchange/mod.rs`) holds a child operator, this
`peer_id`, a `CommunicationInitializer`, an optional `ClusterCom` and a `DistributionFn`. `open`
opens the child and takes this peer's communication handle; `next` calls `send` first and falls
through to `receive` only when `send` returns `None`. `send` pulls records from the child until one
is destined for this peer, returns that one unbuffered, clones the rest to `com.send`, and on child
exhaustion calls `com.close_send()` and returns `None`; `receive` then drains records until
`com.are_all_closed()`. Distribution functions (`distribution/distribution_impl.rs`) are
`record[join_key] rem_euclid peers` for repartitioning, and every peer id for replication.

**Join plans** (`join/join_impl.rs`) are compositions. Symmetric repartitioning wraps *both* children
in exchanges on their join keys and feeds them to a left-built equi-hash join; asymmetric
repartitioning shuffles only the right child; the replication join broadcasts the right child. The
semi-join reduction projects the right child to its join key, repartitions those keys, and uses them
as the right side of a left semi join over the local left child, so fewer left rows cross the
network. All four go through the `TestMockOperatorBuilder` trait (`join/mod.rs`), through which tests
inject real join and projection operators.

**Network** (`src/network.rs`): `ChannelInitializer` hands out one `ChannelClusterCom` per peer over
`mpsc` channels, one channel per peer pair, so a "cluster" is a set of threads in one process.
`receive` blocks until a record arrives or every other peer sent `ClusterMessage::EndOfStream`;
`close_send` broadcasts `EndOfStream` and drops the senders; `send` asserts the destination is not
the sender.

### Bitmap indexing (`src/index/`)

Rows map to bits by one convention (documented in `src/index/mod.rs`): row `R` is bit `R mod 8` of
byte `R / 8`; `IndexIterator` (`index_iterator.rs`) walks a `Vec<u8>` lazily, yielding set bits as
increasing `RowID`s.

- **Naive** (`naive_bitmap_index.rs`): a `HashMap<Value, Vec<u8>>` with one bitmap per distinct key
  value, built over two table scans. `range_lookup` ORs every bitmap whose key falls inside
  `[start, end]`, so cost scales with the distinct values the range covers.
- **Range-encoded** (`range_encoded_bitmap_index.rs`): a `BTreeMap<Value, Vec<u8>>` where entry `v`
  holds the cumulative row set with value *greater or equal* to `v`; construction sets each row's
  bit in every bitmap whose key is `<=` the row's value. Lookup is one pass,
  `bitmap(start) AND NOT bitmap(first key above end)` — two bitmaps whatever the range width.
- **Decomposed** (`decomposed_bitmap_index.rs`): values split into `total_digits` base-`B` digits,
  stored as `vecs[digit][digit_value][byte]`. Lookup decomposes `start` and `end`, walks digits
  most-significant first under a running prefix-equality mask, accumulates rows whose digit is above
  `start`'s (respectively below `end`'s) digit, adds the equal rows, and ANDs the two bounds.
  `determine_optimal_base` picks the base and currently returns the constant `5`.

### Locking (`src/locking/`)

**Lock table** (`lock_table.rs`): `HashMap<RowID, BasicLockState>`, a state being a `HashSet<TID>` of
readers, an `Option<TID>` writer and a `VecDeque<(TID, LockRequest)>` queue. `lock` grants a read
when no writer exists, a write when no reader or writer exists, and upgrades in place when the
requester is the only reader; conflicts go to the back of the queue, and re-requesting a held or
stronger lock returns `LockAlreadyHeld`. `release` drops the caller's lock, then re-runs the queue
head through `lock` until the first request that still fails, returning the granted
`(TID, LockRequest)` pairs, with `check_and_merge` collapsing the duplicate an upgrade produces.

**Multi-granularity locking** (`multi_granularity_locking.rs`): `MultiGranularityLockTree` holds a
root `MGLNode`, each node carrying a `HashSet<(TID, MGLLock)>` and named children. `lock` asks for
`IntentionRead` / `IntentionWrite` on every node but the last and `Read` / `Write` on the last.
`can_grant` decides from the modes present on a node: empty grants, an exclusive lock denies
everything, any write request denies, `IS` grants, `S` is denied only by an existing `IX`, `IX` by an
existing `S`. The path is checked before any lock is inserted, so a denial returns the vector up to
and including `MGLLock::Denied` and leaves the tree untouched. `release(tid)` removes every lock the
transaction holds anywhere in the tree, reporting `LockNotHeld` if it holds none.

### What is original here

Course-provided (marked "will be replaced by the runner"): `Cargo.toml`, `src/lib.rs`, the `mod.rs`
files of `index`, `locking` and `dist_operator`, `src/network.rs`, `src/test_util.rs`,
`src/dist_operator/join/public_test_util.rs`, and the `*_tests_*.rs` harnesses. Written by the
author: `index_iterator.rs`, `naive_bitmap_index.rs`, `range_encoded_bitmap_index.rs`,
`decomposed_bitmap_index.rs`, `lock_table.rs`, `multi_granularity_locking.rs`,
`distribution_impl.rs`, `exchange_impl.rs` and `join_impl.rs`.

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

- **Local records first in `Exchange`.** `next` drains its own child before touching the network and
  returns local records without a receive buffer, keeping the operator pipelined and allocation-light
  — but a peer finishes its local partition before absorbing remote streams.
- **Cumulative bitmaps trade write work for flat query cost.** The range-encoded index writes each
  row's bit into every bitmap at or below its value, so a lookup needs two bitmaps; the naive index
  is cheaper to build but pays per distinct value inside the range.
- **Decomposition trades bitmap count for digit bookkeeping.** Base `B` over `d` digits uses `B * d`
  bitmaps instead of one per distinct value, capping index size on high-cardinality columns at the
  cost of per-digit bound computation under a prefix mask; the base is a real knob.
- **Queued, not aborted, conflicts.** Since deadlock prevention is forbidden, the lock table never
  rejects a conflicting request and wakes only from the queue head, so a blocked writer holds up
  later readers: strict FIFO fairness, no writer starvation, no cycle detection.
- **All-or-nothing MGL acquisition.** Validating the whole path before mutating state keeps the tree
  consistent on failure; the visible consequence is that a request failing deep in the tree releases
  every lock the transaction already held, per the exercise's deny-to-release rule.

## Where to look first

- `src/dist_operator/exchange/exchange_impl.rs` — the `send` / `receive` split and the ordering in
  `next`; `close_send` is reached only once the child returns `None`.
- `src/index/range_encoded_bitmap_index.rs` — the construction loop setting a bit in every bitmap at
  or below the current value, and a lookup resolving bounds by seeking into the `BTreeMap` (it expects
  an entry above `end_key` to subtract with).
- `src/index/decomposed_bitmap_index.rs` — the mirrored lower- and upper-bound loops and the running
  prefix-equality mask; `determine_optimal_base` is the benchmark hook.
- `src/locking/lock_table.rs` — the upgrade branch guarded by `readers.len() == 1`, and `release`,
  which re-validates the queue head by calling `lock` again while `check_and_merge` folds an upgrade
  into one returned grant.
- `src/locking/multi_granularity_locking.rs` — `can_grant`, reducing compatibility to the modes on a
  node, plus the two-pass `lock` and `release_lock_recursive`.
