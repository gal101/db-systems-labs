# radix_join — Cache-Conscious Multi-Threaded Radix Join

A multi-threaded equi-join over 64-bit keys that partitions both input relations by a radix bit count chosen at runtime, so each worker thread touches data sized to fit its share of the L3 cache.

## What it does

- Joins two relations of `tuple_t` (`uint64_t key`, `uint64_t rid`) on key equality and returns `(R.rid, S.rid)` partner pairs through `result_relation_t::data` (`include/Types.hpp`).
- Chooses the radix bit count `B` at runtime from the build-side tuple count and the configured cache budget, so that a *single* partitioning pass already yields buckets small enough for one thread's L3 share (`calculate_radix_bits`, `src/RadixJoin.cpp`).
- Partitions each relation in three phases: per-thread bucket histograms, a global prefix sum that converts histograms into write offsets, then a parallel scatter into one pre-sized, contiguous destination array.
- Joins each bucket pair independently: build a hash table over the build-side bucket (linear probing, table size rounded up to the next power of two ≥ 4 × bucket size), then probe it with the other bucket.
- Distributes buckets across threads with an atomic work counter rather than a fixed static split.
- Merges per-thread join output with a second prefix-sum pass and copies the per-thread vectors into the result array in parallel.

Design intent is throughput-oriented: partitioning is what keeps later probes local, and it is the only place the code reasons about cache size. Measured results are in [Performance](#performance) below; correctness of every run was cross-checked against an independent hash join, and the harness itself is not part of this repository.

### What is original here

This repository mixes course scaffolding with the solution, and the split is visible in the files:

- Scaffolding: `CMakeLists.txt`, the vendored Catch2 single header (`catch/catch.hpp`) and its `catch/main.cpp` entry point, the basic test cases in `test/basic.cpp`, the CI pipeline in `.gitlab-ci.yml`, and the header skeletons `include/Types.hpp`, `include/RadixJoin.hpp`, `include/Config.hpp` — the last declares `Config::L3_CACHE_SIZE` and `Config::NUM_CORES` with their intended values left commented out.
- The solution is `src/RadixJoin.cpp`: the two `Config` static definitions, `calculate_radix_bits`, the `PartitionedRelation` helper type, `partition_relation`, `join_buckets`, and `RadixJoin::join`. The comment block above `join()` restates the task requirements and was part of the given frame.
- Only the basic test set ships here. The CI configuration runs additional test executables and a benchmark that are supplied from outside the repository at CI time, so those cannot be run from this checkout.

## Architecture

`RadixJoin` holds references to the two input relations plus the result object (`include/RadixJoin.hpp`); the whole algorithm is implemented as free functions in `src/RadixJoin.cpp` operating on plain arrays and `std::vector`s.

- **Data model** (`include/Types.hpp`) — `tuple_t` is a 16-byte key/rid pair; `relation_t` owns a raw `tuple_t*` plus a tuple count and frees it in its destructor; `result_relation_t` is a `std::vector<std::pair<uint64_t, uint64_t>>` of rid pairs, with an optional `std::mutex` that the implementation never uses.
- **Configuration** (`include/Config.hpp`, defined at `src/RadixJoin.cpp:5-6`) — `L3_CACHE_SIZE = 24 * 1024 * 1024` bytes and `NUM_CORES = 8`.
- **Radix bit computation** (`calculate_radix_bits`) — estimates the build-side footprint as `num_tuples_R * (sizeof(tuple_t) * 4 + 1)`, i.e. 65 bytes per 16-byte tuple, and doubles the partition count until `size_R / partitions <= L3_CACHE_SIZE / NUM_CORES`. With the configured values that per-thread budget is 3 MiB. The loop stops at `B == 16` at the latest. The decision uses `R`'s tuple count only.
- **Partitioning** (`partition_relation(rel, B)`) — allocates `data` with exactly `rel.number_tuples` tuples up front (no growth, no reallocation) and builds `offsets`, where bucket `b` occupies `data[offsets[b] .. offsets[b+1])`. `B == 0` short-circuits to a plain copy with `offsets = {0, n}`.
  - *Phase 1, histogram:* the input is split into `NUM_CORES` contiguous slices; each thread counts its slice into its own row of a `NUM_CORES * num_buckets` counter array, keyed by `key & mask` where `mask = (1 << B) - 1`. No atomics are needed because the rows are disjoint.
  - *Phase 2, prefix sum:* one thread walks buckets in order and, inside each bucket, threads in order, writing `write_offsets[t * num_buckets + b] = sum` and accumulating `sum`. The per-bucket start offsets land in `res.offsets`.
  - *Phase 3, scatter:* each thread re-walks its original slice and writes `rel.data[i]` to `res.data[write_offsets[t * num_buckets + b]++]`. Because every thread owns a distinct slice of every bucket, destinations never overlap and the scatter is atomic-free.
- **Bucket join** (`join_buckets`) — `NUM_CORES` threads pull bucket indices from `std::atomic<uint32_t> next_bucket` via `fetch_add(1)`, skipping buckets where either side is empty. Per bucket it allocates a hash table of `{key, rid}` pairs and derives the initial slot with `bucket_hash(key, h)` — a multiplicative mix, because the low `B` bits are constant inside a bucket and masking them off directly leaves the table effectively full. It then probes linearly until an empty slot (build) or a match / empty slot (probe). Hits are collected into a per-thread vector.
- **Output merge** — a sequential prefix sum over the per-thread result sizes, then one thread per chunk copying its vector into `out.data`. Bucket scheduling order makes the row order of the result nondeterministic.
- **Entry point** (`RadixJoin::join`) — computes `B` from `R`, partitions `R` on a spawned thread while the calling thread partitions `S`, joins that thread, and hands both partitioned relations to `join_buckets`.

## Provenance and scope

This is the **implementation extract** of a university lab assignment. What you see here
is the part I wrote — the data structures and algorithms described above. It is published
so the design can be read and reviewed.

The surrounding framework is **not** included and is **not** redistributed. The module
skeleton, build files, the course's test harness, its fixtures and the CI configuration
all belong to the course, not to me. Two consequences follow, and both are deliberate:

- **This will not compile or run on its own.** The modules below implement traits, types
  and interfaces that are declared in the omitted files.
- **No test or benchmark artifacts are committed here**, because the harness that produces
  them belongs to the course and is not published. The measurements in
  [Performance](#performance) were taken with a separate harness on my own machine.

Read it as a code sample, not as a runnable project.

## Key design decisions

- **Budget the L3 *share per thread*, not the whole L3.** `L3_CACHE_SIZE / NUM_CORES` (24 MiB / 8 = 3 MiB) is the partition target, because all workers run concurrently and a bucket larger than one thread's share would be evicted by its neighbours. The tradeoff is more buckets than a single-threaded sizing would pick, and the histogram/offset arrays grow as `NUM_CORES * 2^B` — bounded by the `B == 16` cap.
- **One radix pass, enforced by the sizer.** `calculate_radix_bits` solves for the bit count directly instead of partitioning repeatedly, which keeps the input streamed once and copied once. The tradeoff is that the sizing rests on a single estimate of the build side; there is no fallback pass if the assumption is wrong.
- **Atomic-free scatter via per-thread bucket slices.** Offsets are computed per `(thread, bucket)` pair, so each writer owns a disjoint destination range and phase 3 needs no locks or atomics. The cost is a `NUM_CORES × num_buckets` histogram and offset matrix instead of a single `num_buckets` array.
- **Conservative 4× + 1 byte footprint estimate.** The sizer multiplies the tuple size by four (plus a byte) rather than using the bare 16 bytes, biasing toward more, smaller buckets so the working set stays inside the per-thread budget with headroom for the accompanying access structures — at the price of more buckets and a larger bucket count for the same input.
- **Dynamic bucket scheduling.** An atomic counter handing out buckets keeps threads busy when bucket sizes are skewed, which is the norm for real key distributions. Tradeoff: results accumulate per thread and must be merged afterwards, and the row order of the result is not deterministic.

## Where to look first

- `src/RadixJoin.cpp:8-19` — `bucket_hash`: why the bucket-local index must mix the key rather than mask a shifted one, and the one line that does it.
- `src/RadixJoin.cpp:33-49` — `calculate_radix_bits`: the whole cache-sizing policy in a dozen lines, including the `L3_CACHE_SIZE / NUM_CORES` budget and the `B == 16` stop.
- `src/RadixJoin.cpp:51-133` — `partition_relation`: the three phases with their scopes clearly separated (`buckets`, then `write_offsets`, then `copy_data`); note that all three loop bounds are cut the same way (`n * t / T`), which is what makes phase 3 safe without synchronization.
- `src/RadixJoin.cpp:135-222` — `join_buckets`: bucket-level work stealing, the `next_power_of_two ≥ 4 × num_r` table sizing, and the bucket-local hash.
- `src/RadixJoin.cpp:237-258` — `RadixJoin::join`: the two-thread partition of `R` and `S`, and the fact that only `R`'s tuple count drives the bit computation.
- `include/Config.hpp` with `src/RadixJoin.cpp:5-6` — the declared-but-unset statics and the translation unit that actually defines 24 MiB and 8 cores; changing the machine assumptions means changing the definition here.

## Performance

Measured on an AMD Ryzen 9 7945HX, compiled with the project's own flags (`-O3 -DNDEBUG`,
`-mavx512*`, C++20), `NUM_CORES = 8`. The baseline is a single-threaded, non-partitioned
open-addressing hash join with linear probing over the same generated input. Figures are
the best of five runs; throughput counts tuples from both relations.

| tuples per relation | this join | single-threaded hash join | ratio |
|---|---|---|---|
| 1 M | 139 M tuples/s | 70 M tuples/s | 2.0× |
| 4 M | 154 M tuples/s | 55 M tuples/s | 2.8× |
| 10 M | 130 M tuples/s | 51 M tuples/s | 2.5× |

Both joins produced identical result counts at every size; that is the correctness check.

The interesting figure is not the ratio but the flatness. Throughput holds at
130–154 M tuples/s from 1 M to 10 M tuples per relation, while the baseline decays from
70 to 51 M/s as its single hash table outgrows L3. Keeping the bucket tables cache-resident
regardless of input size is the whole point of the partitioning.

One detail that this measurement forced: the bucket-local hash must mix the key. An
earlier revision masked `(key >> B)` directly, which for keys drawn from a range of the
same order as the row count can only address about as many slots as there are rows in the
bucket — an effective load factor near 1.0, where linear probing degenerates into a
quadratic scan. `bucket_hash` fixes that; at a wide keyspace the two versions measure
within noise of each other, which confirms the hash was the only defect.
