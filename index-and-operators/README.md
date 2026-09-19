# SDMS Query Execution Engine — B+Tree Index, K-Way Merge, Volcano Operators

A Rust library implementing the index and operator layers of a relational engine: a unique B+tree
over `i32` keys, an external k-way merge over fixed-capacity pages, and a pull-based (Volcano)
pipeline of table scan, sort and equi-hash-join over a shared page/record model.

## What it does

- **Unique B+tree index** (`src/index/btree.rs`): point lookup, inclusive `range_lookup` that walks
  right-hand leaf siblings instead of re-descending the tree, and `insert` that overwrites the
  stored `RecordID` when the key already exists.
- **Node splitting without deletion**: leaves split in half and promote the new leaf's smallest key,
  inner pages split in half and promote a middle key, and a root split allocates a new root one level
  up. There is no delete or underflow/merge path — the tree only grows.
- **External k-way merge** (`src/kwaymerge/kwmerge.rs`): `sort` splits the page slice into `k` runs
  and recurses; `merge` streams `k` run cursors record-by-record into one output page and records
  each cursor's `(PageID, SlotID)` whenever an output page fills up.
- **Volcano operators** (`src/operator/`): `Box<dyn Operator>` children driven by
  `open` / `next -> Option<Record>` / `close`; `TableScan` streams from a buffer pool, `Sort` blocks
  and materialises its child, `EquiHashJoin` hashes the left child and probes with the right.

## Architecture

**Type and page layer — `src/lib.rs` (provided).** `PageID(u32)` indexes the page store, `SlotID(u16)`
a slot inside a page, and `RecordID { page_id, slot_id }` the physical tuple pointer.
`BasePage<RT, HT, CAP>` keeps `records: [RT; CAP]`, `length` and a per-page `header` by value;
`update` writes one slot, `insert` shifts the tail with `copy_within`. `Value` is the column type
(`Bool`, `Int`, `Varchar`, `PageID`, `SlotID`, `RecordID`).

**Index — `src/index/`.** `UniqueBPlusTree` holds only `root_page: PageID`; every method takes the
page store as `&mut Vec<Page>`, so the tree has no buffer manager and new pages take their ID from
`buffer.len()` at allocation. `InnerPage` is `GenericBTreePage<PageID, (), 508>` (508 child
pointers), `LeafPage` is `GenericBTreePage<RecordID, BTreeHeader, 336>`, and the leaf header carries
`left`/`right` sibling pointers. `Page` is an `#[repr(align(4096))]` enum over the two kinds.

Routing calls `optimized_bsearch_inner`, a `partition_point` over `records[0..length - 1]`: a key is
the *lower bound of the child pointer that follows it*, the leftmost child has no lower bound, and
the last key slot is never consulted. Leaf search is `binary_search_by_key` over the full
`records[0..length]`. Insertion recurses top-down (`recursive_insert`); a full leaf goes to
`split_leaf`, a full inner page to `split_inner`, and `insert` adds a new root when the recursion
returns a promotion. Keys are plain `i32` with the derived `Ord` — no user-supplied comparator.

**K-way merge — `src/kwaymerge/`.** `KwayPage` is `BasePage<i32, u8, 10>`: bare `i32` records, ten per
page, and a page counts as empty when `length == 0`. `sort` copies the input slice and recurses into
each of the `k` runs until a run is already ordered (checked with a test-module helper) or is a
single page. `merge` walks `pages_to_merge: Vec<Option<usize>>` and `curr_slots: Vec<usize>` (one
entry per run), picks the minimum by scanning the active cursors with a strict `<` comparison (ties
go to the lowest run index), appends it to the current output page, and advances that run's page when
its page is exhausted. When an output page reaches capacity it pushes one `RecordID` per still-active
run into `states_at_finalizing` *before* the winning cursor advances; `sort` discards that vector,
direct callers get it back.

**Operators — `src/operator/`.** `Record` is a `Vec<Value>` row with `get`/`len`; `Table` exposes
`page_list()` and `BufferPool` exposes `pin(PageID) -> Box<DummyPage>` / `unpin`, alongside the
`DummyScan`/`DummyTable`/`DummyBufferPool`/`DummyPage` test doubles. Operators hold children as
`Box<dyn Operator>` and forward `open`/`close`, so plans are nested at runtime. `TableScan::next`
pins the current page, clones the slot's record, unpins, and moves to the next page ID when the slot
index passes `length`. `Sort::open` drains the child into a `Vec<Record>`, sorts with the
caller-supplied `fn(&Record, &Record) -> Ordering`, and `next` pops from the end. `EquiHashJoin::open`
fills a `HashMap<Value, Record>` from the left child (unique keys assumed); `next` loops over
right-child records and emits `left values ++ right values` per match, skipping rows without a partner
— an inner join, so unmatched build-side rows are never emitted.

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

- **Pages live in the caller's `Vec<Page>` and the tree stores only `root_page`.** No pinning,
  latching or eviction machinery to build or test, at the cost of two storage abstractions in one
  crate (`&mut Vec<Page>` for the index versus `Rc<RefCell<dyn BufferPool>>` for operators) and no
  durability story for index pages.
- **Fixed-capacity pages via const generics.** Capacities are documented as reducible to even values
  ≥ 4 for local testing while the graded run uses 508/336: page-sized arrays without per-record
  allocation, but `CAPACITY` becomes part of the type and `insert` must memmove the array tail.
- **Inner keys as lower bounds for the following child pointer.** Promoting the new leaf's minimum
  key means no separator keys are duplicated, but the first key of an inner page is dead state and an
  inner page's last key slot is never consulted.
- **Unique keys with overwrite semantics.** `insert` on an existing key rewrites the `RecordID` in
  place, even in an already-full leaf, so no duplicate chain or overflow page exists to maintain.
- **`sort` copies the input slice before recursing.** Run boundaries stay contiguous and the caller's
  pages are untouched, but the full page array is held in memory: the k+1-page discipline is enforced
  only inside `merge`, which is what the merge tests grade directly.
- **No `assert!`/`println!` in the implementation files**; failures use `expect`/`panic!`, because the
  grading pipeline strips those exact lines from non-test sources before compiling.

## Where to look first

- `src/index/btree.rs` — `split_leaf` and `split_inner`: both allocate the sibling with
  `PageID(buffer.len())`, halve `length` and return `(promoted_key, new_page_id)` up the recursion;
  only the `right` link of the split leaf is written, so its right neighbour keeps a stale `left`
  pointer that nothing reads.
- `src/index/btree.rs` — `optimized_bsearch_inner` versus `optimized_bsearch_leaf`: one searches
  `records[0..length - 1]`, the other the full `records[0..length]`; that off-by-one is what makes
  inner keys lower bounds.
- `src/kwaymerge/kwmerge.rs` — `merge`: minimum selection by linear scan with strict `<`,
  `pages_to_merge[i] = None` once a run's next page leaves its `lists_start` block or is empty, and
  the state snapshot taken before the winning cursor advances.
- `src/kwaymerge/kwmerge.rs` — `sort`: the `stepsize == 0` fallback that merges one page per run, and
  the reuse of `is_sorted` from the basic test module as an "already ordered" check.
- `src/operator/` — `table_scan_impl.rs::next` does one `pin`/`unpin` pair per emitted record and
  reads slot 0 of a newly entered page without checking its `length`; `sort_impl.rs` sorts with
  `cmp(b, a)` in `open` and pops from the end in `next`, so the two reversals cancel out.

### What is original here

The solution is the five implementation files: `src/index/btree.rs`, `src/kwaymerge/kwmerge.rs`,
`src/operator/sort/sort_impl.rs`, `src/operator/join/join_impl.rs` and
`src/operator/table_scan/table_scan_impl.rs`. Everything else is course-provided scaffolding:
`src/lib.rs`, the `mod.rs` files (each marked as replaced by the runner), the `*_tests_*.rs`
harnesses, `benches/index_bench.rs`, `sanitize.sh`, the CI configuration, and the operator data model
with its `Dummy*` doubles. The tests are the specification these five files are written against, not
original work.
