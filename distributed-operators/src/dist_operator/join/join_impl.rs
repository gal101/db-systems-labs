use crate::network::CommunicationInitializer;

use crate::DynOperator;
use crate::dist_operator::join::TestMockOperatorBuilder;

/// In this task you are asked to create an Operator for each type of distributed join
/// algorithm seen during the lectures:
/// - Symmetric repartitioning join
/// - Asymmetric repartitioning join
/// - Replication join
/// - Semi-join reduction
///
/// We are, however, not asking you to implement the specific algorithm for them.
/// You just have to leverage one or more functions defined in `mod.rs` to create the join
/// operators.
/// All the join algorithms, except the semi-join, leverage the `build_equi_hash_join_left`
/// function to create the join operator. The parameters that you provide to the function,
/// however, depend on the type of distributed join you are implementing.
///
/// These functions create ONE instance of a join operator. As such, it will have only one left
/// child and one right child. In our tests, we will call the function multiple times to create
/// different parallel instances of the same join operator, each consuming data from different
/// partitions.
/// Each join instance is responsible of a subset of keys, but it is NOT responsible of
/// shuffling keys. Such task is responsibility of the left and right children.
///
/// You can use the following schema to help reasoning:
///
///         [Join_0]                    [Join_1]
///     /             \             /             \
/// [Table_A_0] [Table_B_0]     [Table_A_1] [Table_B_1]
///
/// Join_0 and Join_1 are responsible for a disjointed subset of keys.
/// Table partitions (A_0, A_1, B_0, B_1) can contain any key.

/// Creates and returns a symmetric repartitioning join
pub fn create_symmetric_repartitioning_join(
    left_child: DynOperator,
    right_child: DynOperator,
    left_join_key: usize,
    right_join_key: usize,
    peer_id: u16,
    com_init_left: Box<dyn CommunicationInitializer>,
    com_init_right: Box<dyn CommunicationInitializer>,
    operator_builder: &dyn TestMockOperatorBuilder,
) -> DynOperator {
    let exchange_left = operator_builder.build_exchange(left_child,
                                                        peer_id,
                                                        com_init_left,
                                                        operator_builder.create_repartitioning_distribution(left_join_key));
    let exchange_right = operator_builder.build_exchange(right_child,
                                                         peer_id,
                                                         com_init_right,
                                                         operator_builder.create_repartitioning_distribution(right_join_key));
    operator_builder.build_equi_hash_join_left(exchange_left, exchange_right, left_join_key, right_join_key)
}

/// Creates and returns an asymmetric repartitioning join
/// Right child is re-shuffled
pub fn create_asymmetric_repartitioning_join(
    left_child: DynOperator,
    right_child: DynOperator,
    left_join_key: usize,
    right_join_key: usize,
    peer_id: u16,
    com_init: Box<dyn CommunicationInitializer>,
    operator_builder: &dyn TestMockOperatorBuilder,
) -> DynOperator {
    let exchange_right = operator_builder.build_exchange(right_child,
                                                         peer_id,
                                                         com_init,
                                                         operator_builder.create_repartitioning_distribution(right_join_key));
    operator_builder.build_equi_hash_join_left(left_child, exchange_right, left_join_key, right_join_key)
}

/// Creates and returns a replication join
/// Right child is replicated
pub fn create_replication_join(
    left_child: DynOperator,
    right_child: DynOperator,
    left_join_key: usize,
    right_join_key: usize,
    peer_id: u16,
    com_init: Box<dyn CommunicationInitializer>,
    operator_builder: &dyn TestMockOperatorBuilder,
) -> DynOperator {
    let exchange_right = operator_builder.build_exchange(right_child,
                                                         peer_id,
                                                         com_init,
                                                         operator_builder.create_replication_distribution());
    operator_builder.build_equi_hash_join_left(left_child, exchange_right, left_join_key, right_join_key)
}

/// Creates and returns a semi-join reduction operator
/// You can assume that the right_join_key column contains only unique values
/// The left table is reduced (i.e., only a subset of its row are replicated and sent).
/// The left table is hash-partitioned on the join key.
pub fn create_semi_join_reduction_left(
    left_child: DynOperator,
    right_child: DynOperator,
    left_join_key: usize,
    right_join_key: usize,
    peer_id: u16,
    com_init: Box<dyn CommunicationInitializer>,
    operator_builder: &dyn TestMockOperatorBuilder,
) -> DynOperator {
    let projection = operator_builder.build_projection(right_child, right_join_key);
    let replication = operator_builder.build_exchange(projection, peer_id, com_init, operator_builder.create_repartitioning_distribution(0));

    operator_builder.build_left_semi_join(left_child, replication, left_join_key, 0)
}
