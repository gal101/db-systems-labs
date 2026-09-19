#include "shuffle.hpp"

#include "comm_helper.hpp"
#include "config.hpp"
#include "threadpool.hpp"
#include <iostream>




Shuffle::Shuffle(Row* rows) : rows(rows) {}

// connect to all nodes in network
std::vector<rdma::Connection*> Shuffle::open_conns() {
    auto& cfg = Config::get();
    auto& helper = CommHelper::get();

    std::vector<rdma::Connection*> conns;
    conns.reserve(cfg.num_nodes);

    for (unsigned int i = 0; i < cfg.num_nodes; ++i) {
        conns.push_back(helper.connect_to_node(i));
    }

    return conns;
}

// close all open connections
void Shuffle::close_conns(std::vector<rdma::Connection*>& conns) {
    auto& cfg = Config::get();
    auto& helper = CommHelper::get();
    for (auto*& conn : conns) {
        if (conn) {
            helper.close_connection(conn);
        }
    }
}

// rdma barrier to sync nodes for a ceratin phase -> makes is reusable
void Shuffle::rdma_barrier(size_t phase, const std::vector<rdma::Connection*>& conns) {
    auto& cfg = Config::get();
    auto* mem = reinterpret_cast<char*>(CommHelper::get().get_local_mem());

    size_t aux_data_pos = cfg.mem_size - sizeof(uint64_t) * 3;
    size_t counter_pos = cfg.mem_size - sizeof(uint64_t) * 2;
    size_t ok_pos = cfg.mem_size - sizeof(uint64_t);

    uint64_t* aux = reinterpret_cast<uint64_t*>(mem + aux_data_pos);
    *aux = 0;

    conns[0]->fetch_add(aux, 1, counter_pos, rdma::Flags().signaled());
    conns[0]->sync_signaled(1);

    if (cfg.my_id == 0) {
        volatile uint64_t* counter = reinterpret_cast<volatile uint64_t*>(mem + counter_pos);
        size_t expected = cfg.num_nodes * phase;
        while (*counter < expected) {
            // wait for all nodes to update counter
        }

        uint64_t* send_val = reinterpret_cast<uint64_t*>(mem + aux_data_pos);
        *send_val = phase;

        // notify other nodes
        for (unsigned int i = 1; i < cfg.num_nodes; ++i) {
            conns[i]->write(send_val, sizeof(uint64_t), ok_pos, rdma::Flags().signaled());
            conns[i]->sync_signaled(1);
        }
    } else {
        volatile uint64_t* ok = reinterpret_cast<volatile uint64_t*>(mem + ok_pos);
        while (*ok != phase) {
            // wait for write notification from node 0
        }
    }
}

// do histogram - multithreaded
std::vector<size_t> Shuffle::histogram() {
    auto& cfg = Config::get();
    size_t threads = 4;

    std::vector<size_t> histo(threads * cfg.num_nodes, 0); // flat 2D array
    size_t rows_per_thread = cfg.num_rows / threads + (cfg.num_rows % threads != 0);

    ThreadPool pool;

    pool.parallel_n(threads, [&](std::stop_token, int tid) {
        size_t start = tid * rows_per_thread;
        size_t end = std::min(start + rows_per_thread, (size_t)cfg.num_rows);

        for (size_t i = start; i < end; ++i) {
            size_t part_id = cfg.get_part_id(rows[i].key);
            size_t node_id = cfg.part_to_node_id(part_id);

            size_t index = tid * cfg.num_nodes + node_id;

            histo[index]++;
        }
    });
    pool.join();

    return histo;   
}

// partition local data
std::vector<size_t> Shuffle::partition_data(const std::vector<size_t>& histo) {
    auto& cfg = Config::get();
    size_t threads = 4;

    // get total rows per target node
    std::vector<size_t> node_totals(cfg.num_nodes, 0);
    for (size_t tid = 0; tid < threads; ++tid) {
        for (size_t node = 0; node < cfg.num_nodes; ++node) {
            size_t index = tid * cfg.num_nodes + node;

            node_totals[node] += histo[index];
        }
    }

    // calculate memory positions for each node
    std::vector<size_t> node_pos(cfg.num_nodes, 0);
    size_t pos = cfg.num_rows * sizeof(Row);

    for (size_t node = 0; node < cfg.num_nodes; ++node) {
        if (node != cfg.my_id) {
            node_pos[node] = pos;
            pos += node_totals[node] * sizeof(Row);
        }
    }
    node_pos[cfg.my_id] = pos;

    // calculate write offsets for each thread and node
    std::vector<size_t> thread_pos(threads * cfg.num_nodes, 0);
    for (size_t node = 0; node < cfg.num_nodes; ++node) {
        size_t curr_pos = node_pos[node];

        for (size_t tid = 0; tid < threads; ++tid) {
            size_t index = tid * cfg.num_nodes + node;

            thread_pos[index] = curr_pos;
            curr_pos += histo[index] * sizeof(Row);
        }
    }

    char* mem = reinterpret_cast<char*>(CommHelper::get().get_local_mem());
    size_t rows_per_thread = cfg.num_rows / threads + (cfg.num_rows % threads != 0);

    // parallel copy to memory
    ThreadPool pool;
    pool.parallel_n(threads, [&](std::stop_token, int tid) {
        size_t start = tid * rows_per_thread;
        size_t end = std::min(start + rows_per_thread, (size_t)cfg.num_rows);

        std::vector<size_t> local_pos(cfg.num_nodes);
        for (size_t node = 0; node < cfg.num_nodes; ++node) {
            size_t index = tid * cfg.num_nodes + node;

            local_pos[node] = thread_pos[index]; // copy the offsets for this thread
        }

        for (size_t i = start; i < end; ++i) {
            size_t part_id = cfg.get_part_id(rows[i].key);
            size_t node_id = cfg.part_to_node_id(part_id);

            Row* dest = reinterpret_cast<Row*>(mem + local_pos[node_id]);
            *dest = rows[i];
            local_pos[node_id] += sizeof(Row);
        }
    });
    pool.join();
    return node_pos;
}

// send data over rdma
void Shuffle::transfer_data(const std::vector<rdma::Connection*>& conns, const std::vector<size_t>& node_pos) {
    auto& cfg = Config::get();
    char* mem = reinterpret_cast<char*>(CommHelper::get().get_local_mem());

    size_t target_pos = 2 * cfg.num_rows * sizeof(Row);
    size_t aux_data_pos = cfg.mem_size - sizeof(uint64_t) * 3;
    size_t transfer_size_pos = cfg.mem_size - sizeof(uint64_t) * 4;

    for (size_t node = 0; node < cfg.num_nodes; ++node) {
        if (node != cfg.my_id) {
            size_t send_bytes = (cfg.my_id == 0) ? (node_pos[0] - node_pos[1]) : (node_pos[1] - node_pos[0]);
            size_t send_rows = send_bytes / sizeof(Row);

            // send transfer_size (number of rows) to other'snode memory first
            uint64_t* size_buf = reinterpret_cast<uint64_t*>(mem + aux_data_pos);
            *size_buf = send_rows;

            // send transfer_size to target node
            conns[node]->write(size_buf, sizeof(uint64_t), transfer_size_pos, rdma::Flags().signaled());
            conns[node]->sync_signaled(1);

            if (send_bytes > 0) {
                conns[node]->write(mem + node_pos[node], send_bytes, target_pos, rdma::Flags().signaled());
                conns[node]->sync_signaled(1);
            }
        }
    }
}

std::span<Row> Shuffle::run() {
    auto& cfg = Config::get();
    char* mem = reinterpret_cast<char*>(CommHelper::get().get_local_mem());

    // clear transfer size metadata
    size_t transfer_size_pos = cfg.mem_size - sizeof(uint64_t) * 4;
    volatile uint64_t* transfer_size_ptr = reinterpret_cast<volatile uint64_t*>(mem + transfer_size_pos);
    *transfer_size_ptr = 0;

    // open rdma connections to nodes
    auto conns = open_conns();
    rdma_barrier(1, conns);

    // build histogram in parallel
    auto histo = histogram();

    auto node_pos = partition_data(histo);
    rdma_barrier(2, conns);

    transfer_data(conns, node_pos);
    rdma_barrier(3, conns);

    
    // figure out result: local and incoming rows
    size_t local_count = (2 * cfg.num_rows * sizeof(Row) - node_pos[cfg.my_id]) / sizeof(Row);
    volatile size_t* transfer_size = reinterpret_cast<volatile size_t*>(mem + transfer_size_pos);
    size_t incoming_count = *transfer_size;

    size_t total_rows = local_count + incoming_count;
    Row* result_ptr = reinterpret_cast<Row*>(mem + node_pos[cfg.my_id]);


    close_conns(conns);

    return std::span<Row>(result_ptr, total_rows);
}
