use std::collections::HashMap;
use crate::Value;
use crate::operator::{Operator, Record};

/// An equi-hash-join for equality joins of two tables.
/// The left child is guaranteed to only contain unique join keys, and the hash-table
/// should always be created using this side (left = build side).
pub struct EquiHashJoin {
    left_child: Box<dyn Operator>,
    /// The column index of the join key in the left child output.
    left_join_key: usize,
    right_child: Box<dyn Operator>,
    /// The column index of the join key in the right child output.
    right_join_key: usize,
    // Add additional fields here if needed.
    is_open: bool,
    hash_map: HashMap<Value, Record>
}

impl EquiHashJoin {
    pub fn new(
        left_child: Box<dyn Operator>,
        right_child: Box<dyn Operator>,
        left_join_key: usize,
        right_join_key: usize,
    ) -> Self {
        Self {
            left_child,
            left_join_key,
            right_child,
            right_join_key,
            is_open: false,
            hash_map: HashMap::new()
        }
    }
}

impl Operator for EquiHashJoin {
    /// Open the child operators and build the hash table.
    fn open(&mut self) {
        self.is_open = true;
        self.left_child.open();
        self.right_child.open();

        while let Some(record) = self.left_child.next() {
            let key = &record.record[self.left_join_key];
            self.hash_map.insert(key.clone(), record);
        }
    }

    /// Emit the next joined tuple or None if there are no more join partners.
    /// A joined tuple is a record of the concatenated values of the two joined rows (left, then right values).
    fn next(&mut self) -> Option<Record> {
        if !self.is_open {
            panic!();
        }

        loop {
            let elem = self.right_child.next();

            match elem {
                Some(mut record) => {
                    let key = &record.record[self.right_join_key];
                    if self.hash_map.contains_key(key) {
                        let mut left_record = self.hash_map.get(key).unwrap().clone().record;
                        left_record.append(&mut record.record);
                        return Some(Record::from(left_record))
                    }
                },
                None => return None
            }
        }
    }

    fn close(&mut self) {
        self.left_child.close();
        self.right_child.close();
        self.is_open = false;
        self.hash_map.clear();
    }
}
