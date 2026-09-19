#include "RadixJoin.hpp"
#include <iostream>
#include <thread>

uint32_t Config::L3_CACHE_SIZE = 24 * 1024 * 1024;
uint32_t Config::NUM_CORES = 8;

RadixJoin::RadixJoin(relation_t &R, relation_t &S) : R_(R), S_(S){
}

RadixJoin::~RadixJoin(){
}


struct PartitionedRelation {
    std::vector<tuple_t> data;
    std::vector<uint64_t> offsets; // bucket i starts from index offsets[i]
};

uint32_t calculate_radix_bits(uint64_t num_tuples_R) {
    uint64_t size_R = num_tuples_R * (sizeof(tuple_t) * 4 + 1);

    uint64_t thread_cache_size = Config::L3_CACHE_SIZE/Config::NUM_CORES;

    uint32_t B = 0;
    uint32_t partitions = 1 << B;

    while(size_R / partitions > thread_cache_size) {
        B++;
        partitions <<= 1;

        if(B == 16)
            break;
    }
    return B;
}

PartitionedRelation partition_relation(const relation_t &rel, uint32_t B) {
    PartitionedRelation res;

    res.data.resize(rel.number_tuples);

    if(B == 0) {
        std::copy(rel.data, rel.data + rel.number_tuples, res.data.begin());
        res.offsets = {0, rel.number_tuples};

        return res;
    }

    uint32_t T = Config::NUM_CORES;
    uint32_t num_buckets = 1 << B;

    std::vector<uint64_t> buckets(T * num_buckets, 0);
    
    auto compute_bucket_sizes = [&](uint32_t thread_id, uint64_t start, uint64_t end) {
        uint64_t mask = (1 << B) - 1;

        for (uint64_t i = start; i < end; ++i) {
            uint64_t key = rel.data[i].key;
            uint32_t bucket = key & mask;

            buckets[thread_id * num_buckets + bucket]++;
        }
    };

    std::vector<std::thread> threads;

    for(uint32_t t = 0; t < T; ++t) {
        uint64_t start = (rel.number_tuples * t) / T;
        uint64_t end = (rel.number_tuples * (t + 1)) / T;

        threads.push_back(std::thread(compute_bucket_sizes, t, start, end));
    }

    for (auto &thread : threads) {
        thread.join();
    }

    std::vector<uint64_t> write_offsets(T * num_buckets, 0);
    uint64_t sum = 0;

    for(uint32_t b = 0; b < num_buckets; ++b) {        
        res.offsets.push_back(sum);
        for(uint32_t t = 0; t < T; ++t) {
            write_offsets[t * num_buckets + b] = sum;
            sum += buckets[t * num_buckets + b];
        }
    }

    res.offsets.push_back(sum);

    auto copy_data = [&](uint32_t thread_id, uint64_t start, uint64_t end) {
        uint64_t mask = (1 << B) - 1;

        for (uint64_t i = start; i < end; ++i) {
            uint64_t key = rel.data[i].key;
            uint32_t bucket = key & mask;

            uint64_t idx = thread_id * num_buckets + bucket;
            uint64_t w_offset = write_offsets[idx];

            res.data[w_offset] = rel.data[i];
            write_offsets[idx]++;
        }
    };

    threads.clear();

    for(uint32_t t = 0; t < T; ++t) {
        uint64_t start = (rel.number_tuples * t) / T;
        uint64_t end = (rel.number_tuples * (t + 1)) / T;

        threads.push_back(std::thread(copy_data, t, start, end));
    }

    for (auto &thread : threads) {
        thread.join();
    }
    return res;
}

void join_buckets(const PartitionedRelation &R_part,
                const PartitionedRelation &S_part,
                uint32_t B,
                result_relation_t &out) {

    uint32_t T = Config::NUM_CORES;
    uint32_t num_buckets = 1 << B;
    std::vector<std::vector<std::pair<uint64_t, uint64_t>>> thread_results(T);

    std::atomic<uint32_t> next_bucket{0};
    std::vector<std::thread> threads;

    for (uint32_t t = 0; t < T; ++t) {
        threads.push_back(std::thread([&, t]() {

            while (true) {
                uint32_t b = next_bucket.fetch_add(1); // get next available bucket
                if (b >= num_buckets) break;

                uint64_t r_start = R_part.offsets[b];
                uint64_t r_end   = R_part.offsets[b+1];
                uint64_t num_r   = r_end - r_start;
                if(num_r == 0) continue;

                uint64_t s_start = S_part.offsets[b];
                uint64_t s_end   = S_part.offsets[b+1];
                uint64_t num_s   = s_end - s_start;
                if(num_s == 0) continue;

                // get hash table size
                uint64_t h = 1;
                while(h < num_r * 4) {
                    h <<= 1;
                }
                //alocate hash table
                std::vector<std::pair<uint64_t, uint64_t>> hash_table(h, {UINT64_MAX, 0});
                // build hash table
                for(uint64_t i = r_start; i < r_end; ++i) {
                    uint64_t key = R_part.data[i].key;
                    uint64_t rid = R_part.data[i].rid;

                    uint32_t hash = (key >> B) & (h - 1);
                    while(hash_table[hash].first != UINT64_MAX) {
                        hash = (hash + 1) & (h - 1);
                    }
                    hash_table[hash] = {key, rid};
                }

                // probe hash table
                for(uint64_t i = s_start; i < s_end; ++i) {
                    uint64_t key = S_part.data[i].key;
                    uint64_t rid = S_part.data[i].rid;

                    uint32_t hash = (key >> B) & (h - 1);
                    while(hash_table[hash].first != UINT64_MAX) {
                        if(hash_table[hash].first == key) {
                            thread_results[t].emplace_back(hash_table[hash].second, rid);
                            break;
                        }
                        hash = (hash + 1) & (h - 1);
                    }
                }
            }
        }));
    }

    for (auto &thread : threads) {
        thread.join();
    }

    uint32_t sum = 0;
    std::vector<uint32_t> offsets;
    for(uint32_t t = 0; t < T; ++t) {
        offsets.push_back(sum);
        sum += thread_results[t].size();
    }

    out.data.resize(sum);
    threads.clear();
    for(uint32_t t = 0; t < T; ++t) {
        threads.push_back(std::thread([&, t]() {
            std::copy(thread_results[t].begin(), thread_results[t].end(), out.data.begin() + offsets[t]);
        }));
    }

    for (auto &thread : threads) {
        thread.join();
    }
}



/*
*RADIX JOIN - Implement Radix join with the following requirements
* 1. Multithreaded
* 2. Use Radix Partitioning to create chunks fitting into the cache, but only one pass needed, i.e., figure out how
*    many bits you need to create N partitions
* 
* Input: Use Member Variables R and S
*
* Return: result_relation_t - defined in Types.hpp
*/
result_relation_t &RadixJoin::join(){
    // find number of bits
    uint32_t B = calculate_radix_bits(R_.number_tuples);

    // partition both relations
    //PartitionedRelation R_part = partition_relation(R_, B);
    //PartitionedRelation S_part = partition_relation(S_, B);

    PartitionedRelation R_part, S_part;

    std::thread t([&]() {
        R_part = partition_relation(R_, B);
    });
    S_part = partition_relation(S_, B);
    t.join();

    // join on the partitioned buckets
    join_buckets(R_part, S_part, B, result);

    return result;
}



