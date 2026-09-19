#pragma once

#include "rdmapp/rdma.hpp"
#include "types.hpp"

#include <span>
#include <vector>




class Shuffle {
    Row* rows;
    std::vector<rdma::Connection*> open_conns();
    void close_conns(std::vector<rdma::Connection*>& conns);
    void rdma_barrier(size_t phase, const std::vector<rdma::Connection*>& conns);
    std::vector<size_t> histogram();
    std::vector<size_t> partition_data(const std::vector<size_t>& hist);
    void transfer_data(const std::vector<rdma::Connection*>& conns, const std::vector<size_t>& node_pos);
public:
    /**
     * rows is a pointer to RDMA registered memory of size cfg.mem_size.
     * The first cfg.num_rows*sizeof(Row) bytes are filled with the local tuples before the shuffle.
     * The remaining bytes (up to cfg.mem_size) can be used by you, for data structures of your choice,
     * as well as the local tuples after the shuffle. 
     */
    Shuffle(Row* rows);

    /**
     * Shuffle the rows according to the part_id and node_id of the row's key.
     * Return the ptr to where the local rows start after shuffling, as well as the number of local rows.
     */
    std::span<Row> run();
};