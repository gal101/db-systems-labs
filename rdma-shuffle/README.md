# Distributed RDMA Shuffle

A distributed, key-partitioned shuffle operator: every node reorders its own rows into per-destination zones and exchanges them with the other nodes through one-sided RDMA writes, synchronising the phases with RDMA atomic operations instead of a software rendezvous.

## What it does

- Assigns every row a partition and a destination node with `Config::get_part_id(key) = key % num_partitions` and `Config::part_to_node_id(part) = part % num_nodes` (`src/config.hpp`).
- Counts, in parallel, how many local rows belong to each destination node, then scatters the rows into per-destination byte ranges inside one pre-registered RDMA buffer (`Shuffle::histogram`, `Shuffle::partition_data` in `src/shuffle.cpp`).
- Ships those ranges to their destination with one-sided `write` verbs into a fixed offset of the peer's registered memory; the receiving process posts no receive work requests and never moves the payload with the CPU — the remote NIC DMAs the bytes into its buffer (`Shuffle::transfer_data`).
- Keeps the nodes in step between phases with a barrier built from a remote `fetch_add` on a counter plus a phase flag written back to each peer (`Shuffle::rdma_barrier`).
- Returns the node's post-shuffle partition as a `std::span<Row>` pointing directly into registered memory: the local partition and the inbound rows are laid out adjacently, so assembling the result costs no copy (`Shuffle::run`).
- The bundled test programs verify the invariant that every returned row satisfies `my_id == part_to_node_id(get_part_id(row.key))` and that the returned row count matches the expected count for that node (`tests/basic_1.cpp`, `tests/basic_2.cpp`, `tests/basic_3.cpp`).

A row is 16 bytes: `struct Row { uint64_t key; uint64_t value; }` (`src/types.hpp`).

## Architecture

`Config` (`src/config.hpp`) is a singleton holding the runtime parameters — `rdma_port`, `my_id`, `num_nodes`, `num_partitions`, `num_rows` (per node), `mem_size`, `node_ips` — parsed by `cli::Parser` (`src/cli_parser.hpp` / `src/cli_parser.cpp`) from `--name value` argument pairs, with every parameter required.

`CommHelper` (`src/comm_helper.hpp`) owns the RDMA plumbing: it `malloc`s one `cfg.mem_size` region, registers it with the `rdmapp` library, starts the listener (`listen(CLOSE_AFTER_LAST | IN_BACKGROUND)`), dials peers via `connect_to_node` (retrying for at most 5 s before throwing), closes connections, and in its destructor waits for the library's server thread and frees the buffer.

`Shuffle` (`src/shuffle.hpp` / `src/shuffle.cpp`) holds a pointer into the first `cfg.num_rows * sizeof(Row)` bytes of that region — the input rows. One `run()` executes:

1. `open_conns()` dials every node id `0 .. num_nodes-1`; node 0 therefore also opens a loopback connection to itself, which lets the barrier always address node 0 through `conns[0]`.
2. `rdma_barrier(1, conns)` — every node confirms it has its listener and buffer up before the shuffle starts.
3. `histogram()` — 4 threads each accumulate per-destination counts into their own row of a `threads × num_nodes` counter array; no atomics, no locking.
4. `partition_data(histo)` — sums the per-thread counters into per-node totals, hands each `(thread, destination)` pair a disjoint byte range after the input rows, and scatters rows there in parallel. It returns `node_pos[]`, the start offset of each destination zone; this node's own zone is placed after all outgoing zones.
5. `rdma_barrier(2, conns)`.
6. `transfer_data(conns, node_pos)` — for each peer: write an 8-byte row count into the peer's metadata slot, then write the zone bytes to offset `2 * num_rows * sizeof(Row)` in the peer's buffer. Each write is signaled and waited individually (`sync_signaled(1)`).
7. `rdma_barrier(3, conns)` — no node reads inbound rows before every node has finished writing.
8. `run()` computes `local_count` from `node_pos[my_id]` up to `2 * num_rows * sizeof(Row)` (where inbound rows begin), reads `incoming_count` from the metadata slot the peers wrote, closes the connections, and returns a span covering both.

The registered region is laid out by convention (offsets in bytes, `R = sizeof(Row)`):

| Range | Contents |
| --- | --- |
| `[0, num_rows*R)` | input rows, written by the test harness before `run()` |
| `[num_rows*R, node_pos[my_id])` | outgoing zones, one contiguous zone per peer (`node_pos[]`) |
| `[node_pos[my_id], 2*num_rows*R)` | this node's own post-shuffle partition |
| `[2*num_rows*R, ...)` | rows written in by peers |
| `mem_size - 4*8` | inbound row count (written by peers) |
| `mem_size - 3*8` | barrier scratch slot, reused as the outgoing size mailbox |
| `mem_size - 2*8` | barrier counter, aggregated in node 0's memory |
| `mem_size - 1*8` | barrier phase flag, written by node 0 |

`ThreadPool` (`src/threadpool.hpp`) provides `parallel_n(n, fn)` over `std::jthread`s plus `join()`; the shuffle uses a fixed 4 threads for both parallel passes.

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

- One registered region for everything (input rows, outgoing zones, inbound zones, 32 bytes of metadata), with the inbound rows landing immediately after the node's own partition. Single registration and a copy-free result — `run()` can return one contiguous `std::span` over DMA memory — at the cost of the offsets being a fixed protocol between sender and receiver rather than something negotiated at connect time, and of the ordering inside `partition_data` becoming load-bearing: move where the local zone is placed and the result assembly breaks.
- Lock-free parallel repacking: per-thread counters plus one disjoint output range per `(thread, destination)` pair, so 4 threads count and scatter without synchronisation. The tradeoff is a second pass over the counts to turn them into offsets, and a thread count fixed at 4.
- The payload moves with one-sided writes and the receiving node posts no work requests, but each transfer is signaled and waited on individually rather than batched behind one completion. The receiving CPU does nothing per transfer; the price is a completion round trip per message.
- A barrier made of RDMA atomics rather than a software rendezvous: every node increments a counter in node 0's memory with `fetch_add`, and node 0 writes the phase number into each peer's flag slot. Phases are counted, so the same memory is reused without allocation and a stale flag cannot pass the barrier — but node 0 is a hot spot (N inbound atomics, N-1 outbound writes), and the counter only ever grows, which is why one `run()` per process is what the code supports.
- The exchange step is sized for the two-node configuration the runner uses: `transfer_data` derives the byte count from `node_pos[0]` and `node_pos[1]` alone and every sender targets the same fixed destination offset. The histogram and partitioning code is node-count agnostic, but 3+ nodes would need the exchange generalised (per-sender offsets and per-peer sizes) to be correct.

## Where to look first

- `src/shuffle.cpp:40-75` — `rdma_barrier`. The entire synchronisation story: `conns[0]->fetch_add(...)` always targets node 0, the local `*aux` receives the previous remote value and is thrown away, and `expected = num_nodes * phase` works only because the counter is never reset.
- `src/shuffle.cpp:106-172` — `partition_data`. How the histogram becomes `node_pos[]` (zone offsets per destination) and then `thread_pos[]` (a disjoint slice inside each zone per thread); note that the local node's zone is appended after every outgoing zone.
- `src/shuffle.cpp:175-237` — `transfer_data` and `run`. The two-write sequence per peer (count, then payload), and how `local_count` and the peer-written `incoming_count` are combined into the returned span. The inbound-count slot is zeroed locally before the first barrier and is thereafter only ever written by peers.
- `src/comm_helper.hpp` — the connection lifecycle. The buffer is a plain `malloc` that is registered explicitly, and the destructor's `server->wait()` is the reason leaked connections hang the program instead of failing loudly.
- `tests/basic_3.cpp` — keys are `i % 10000`, so keys repeat and destinations are unevenly loaded, which is the distribution the partitioning has to survive; `basic_1` and `basic_2` use distinct keys `0 .. num_rows-1`.
