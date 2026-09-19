use crate::{Record, Value};

use super::DistributionFn;

/// Creates a hash distribution function
///
/// All keys are of the type [`Value::Int``]
///
/// Hash function:
/// 'receiving node' = 'key' mod 'number of peers'
///
/// `join_key` is the index of the join key in the `record` vector (i.e., which
/// field in `record` is the actual join key). It is NOT the value of the join key.
pub fn create_repartitioning_distribution(join_key: usize) -> DistributionFn {
    Box::new(move |record: &Record, peers| {
        let Value::Int(number) = record.get(join_key).unwrap() else {panic!()};
        let result = number.rem_euclid(peers as i32) as u16;
        vec![result; 1]
    })
}

/// Creates a replication distribution function
///
/// All keys are of the type [`Value::Int``] and are send to every other node
pub fn create_replication_distribution() -> DistributionFn {
    Box::new(move |_: &Record, peers| (0..peers).collect())
}
