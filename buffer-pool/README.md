# SDMS Storage Engine: Disk Manager and Buffer Pool Manager

The bottom two layers of a relational storage engine, written from scratch in Rust for the Scalable Data Management Systems (SDMS) course at TU Darmstadt: a file-backed 4 KiB page store, and an in-memory buffer pool that caches those pages.

## What it does

- Maps every database page to a fixed-size 4096-byte block (`PAGE_SIZE = 4 * KIBI_BYTES`) inside a single database file, addressed by arithmetic alone: byte offset `page_id * PAGE_SIZE`.
- Allocates page IDs from a monotonic high-water mark (`next_free`) and recycles freed IDs through a FIFO free list (`free_list: VecDeque<PageID>`), so a freed page is reused before the file grows.
- Rejects misuse instead of corrupting state: reading, writing or freeing a page that is on the free list or at/above the high-water mark returns `DiskManagerError::InvalidPageID`, which also makes double-frees detectable.
- Caches up to `BUFFER_POOL_SIZE` (1024) pages in memory, translating `PageID -> FrameID` through a hash table and handing callers a mutable reference to the cached page.
- Tracks per-frame metadata (pin count, dirty bit, replacement state) and writes a dirty page back to disk before its frame is reused for another page.
- Implements two interchangeable eviction policies, LRU and CLOCK, behind `ReplacementStrategyTrait` and selected by the type parameter of `BufferManager`.

### What is original here

Files carrying `// This file will be replaced by the runner` are course-provided scaffolding: `src/lib.rs` (constants, `PageID`/`FrameID` newtypes), the module files `src/disk/mod.rs` and `src/buffer/mod.rs` (public types, traits, error enums, in-memory `DummyDiskManager`), `src/buffer/frame_pool.rs` (the `FramePool<T>` index wrapper), and all four `*_tests_*.rs` files. The implementation written here lives in the two files with no runner marker: `src/disk/disk_manager.rs` (`new`, `allocate`, `free`, `read`, `write`) and `src/buffer/buffer_manager.rs` (`FrameDescriptor` fields, `LRUReplacementStrategy`, `ClockReplacementStrategy`, `BufferManager::new`, `pin`, `unpin`). The struct skeletons, signatures and doc comments in those two files were given; the bodies are mine.

## Architecture

Two layers, both generic, each testable without the other:

**Disk layer** — `src/disk/disk_manager.rs`. `DiskManager` owns an open `File`, the `next_free` high-water mark and the `free_list`. A `RawPage` is a plain `[u8; PAGE_SIZE]`; `read` and `write` seek to `page_id.0 * PAGE_SIZE` and issue one `File::read`/`File::write`. `src/disk/mod.rs` supplies `DiskManagerError` and `DiskManagerTrait` (`read`/`write` over a `MaterializedPage`), which exists so the buffer manager can be exercised against an in-memory fake instead of real files.

**Buffer layer** — `src/buffer/buffer_manager.rs`. `BufferManager<DiskManager, ReplacementStrategy>` holds a shared `Rc<RefCell<DiskManager>>`, a `page_table: HashMap<PageID, FrameID>` recording which pages are resident, a `FramePool<FrameDescriptor>` for per-frame metadata, a `FramePool<MaterializedPage>` holding the cached bytes, a `buffer_count` of occupied frames, and `last_evict`.

Flow through `pin(pid)`:

1. Hit in `page_table`: the resident frame is reused, `on_pin` updates the strategy state, `pin_count` increments.
2. Miss with a non-full pool: the frame at index `buffer_count` is claimed sequentially.
3. Miss with a full pool: `replacement_strat.replace(&mut frame_descriptors)` returns a victim `PageID`; if that frame is dirty it is flushed via `DiskManager::write` first, the victim is removed from `page_table`, and `last_evict` records it.
4. The frame is relabelled with `pid`, `DiskManager::read` fills it, `on_pin` runs, `pin_count` becomes 1.

`unpin(pid, dirty)` looks the frame up (panicking if the page is not resident), assigns the dirty bit, and decrements the pin count. A page that has been read once keeps occupying its frame until some other page evicts it.

Page representation differs between the layers: on disk a page is a bare 4096-byte `RawPage` at a computed offset, while in memory it is a `MaterializedPage` — a `PageID` header plus `[u8; DATA_SIZE]`, where `DATA_SIZE = PAGE_SIZE - size_of::<PageID>()` (4088 bytes on a 64-bit target).

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

- **`PageID(0)` is reserved as a sentinel.** Page IDs start at 1 (the on-disk file's first block is never allocated), which is what lets `last_evict` be initialised to `PageID(0)` and mean "nothing has been evicted yet", instead of paying for an `Option<PageID>`. The cost is one unusable page slot.
- **Newtypes plus a typed pool, not type aliases.** `PageID(usize)` and `FrameID(usize)` are distinct types, and `FramePool<T>` implements `Index`/`IndexMut` for both `FrameID` and `&FrameID`, so a frame index cannot be passed where a page ID is expected. This costs boilerplate (four `Index` impls), and buys compile-time elimination of the classic on-disk/in-memory ID mix-up.
- **Generic over `DiskManagerTrait` and `ReplacementStrategyTrait`, with `Rc<RefCell<DiskManager>>`.** Tests can swap in the in-memory `DummyDiskManager` and share one disk between two buffer managers; the price is runtime borrow checking and a single-threaded, non-`Send` buffer manager.
- **Write-back on eviction, not write-through.** `unpin` only sets a bit; the `DiskManager::write` happens later, when the frame is recycled. Many modifications to one page therefore cost one write, but a dirty page can live in memory indefinitely, and the code issues no `fsync`/`sync_all`, so durability ends at the operating system's page cache.
- **FIFO free list and an O(n) victim search.** `VecDeque` gives O(1) front-pop allocation and back-push freeing, at the cost of a linear `contains` check in the validity guards and of always handing out the oldest freed ID. Likewise LRU scans all frame descriptors for the minimum timestamp rather than maintaining an ordered list — simpler and pool-size independent, but linear in `BUFFER_POOL_SIZE` per eviction.

## Where to look first

- `src/buffer/buffer_manager.rs`, `BufferManager::pin` — the eviction path in one place: dirty write-back before the read, removal from `page_table`, the `last_evict` bookkeeping, and the `AllPagesPinned` error when no unpinned victim exists.
- `LRUReplacementStrategy::on_pin` / `replace` — the timestamp is stamped and then incremented, so the counter is a strict ordering; `replace` picks the unpinned frame with the smallest value and treats `global_time` as the initial upper bound.
- `ClockReplacementStrategy::replace` — the sweep is bounded by `2 * fds.len()` starting at `clock_hand`; frames that are still pinned are skipped without losing their reference bit, and the hand is advanced past the victim on the way out.
- `src/disk/disk_manager.rs`, `free` / `read` / `write` — the same two-part guard (`free_list.contains(&page_id) || page_id.0 >= next_free.0`) enforces the allocation model at every entry point.
- `src/lib.rs` — the two constants that set the whole geometry (`PAGE_SIZE`, `BUFFER_POOL_SIZE`) and the newtype definitions that carry the ID scheme.
