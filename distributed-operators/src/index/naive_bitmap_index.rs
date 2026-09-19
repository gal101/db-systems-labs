use crate::Operator;
use crate::Value;
use crate::index::index_iterator::IndexIterator;
use crate::index::shared::*;
use crate::index::{BitmapIndex, NaiveBitmapIndex};
use crate::test_util::{DummyTable, TableScan};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

impl NaiveBitmapIndex {
    /// Create a naive bitmap index for the records in table,
    /// using the column key_column_number.
    pub fn new(table: Arc<DummyTable>, key_column_number: usize) -> Self {
        let length = table.get_number_of_records();


        let mut table_scan = TableScan::new(table.clone());
        table_scan.open();

        let unique_values: HashSet<Value> = table_scan.into_iter().map(|record | record.get(key_column_number).unwrap().clone()).collect();

        let mut hash_map : HashMap<Value, Vec<u8>> = HashMap::new();

        for value in unique_values.iter() {
            let vec_len = (length + 7) / 8;
            let vec: Vec<u8> = vec![0; vec_len];
            hash_map.insert(*value, vec);
        }

        let mut table_scan = TableScan::new(table.clone());
        table_scan.open();
        for (index, record) in table_scan.into_iter().enumerate() {
            let value = record.get(key_column_number).unwrap();
            let vec = hash_map.get_mut(value).unwrap();

            let page_index = index / 8;
            let bit_index = index % 8;

            vec[page_index] |= 1 << bit_index;
        }

        Self {
            number_of_records: length,
            hash_map
        }
    }
}

impl BitmapIndex for NaiveBitmapIndex {
    fn range_lookup(&self, start_key: Value, end_key: Value) -> IndexIterator {
        let vec_len = (self.number_of_records + 7) / 8;
        let mut bitmap: Vec<u8> = vec![0; vec_len];

        for (value, vec) in self.hash_map.iter() {
            if *value >= start_key && *value <= end_key {
                for (i, byte) in bitmap.iter_mut().enumerate() {
                    *byte |= vec[i];
                }
            }
        }

        IndexIterator::new(bitmap)
    }
}
