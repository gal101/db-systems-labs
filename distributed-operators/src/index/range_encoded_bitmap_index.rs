use crate::Operator;
use crate::Value;
use crate::index::index_iterator::IndexIterator;
use crate::index::shared::*;
use crate::index::{BitmapIndex, RangeEncodedBitmapIndex};
use crate::test_util::{DummyTable, TableScan};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

impl RangeEncodedBitmapIndex {
    /// Create a range encoded bitmap index for the records in table,
    /// using the column key_column_number.
    pub fn new(table: Arc<DummyTable>, key_column_number: usize) -> RangeEncodedBitmapIndex {
        let length = table.get_number_of_records();


        let mut table_scan = TableScan::new(table.clone());
        table_scan.open();

        let unique_values: HashSet<Value> = table_scan.into_iter().map(|record | record.get(key_column_number).unwrap().clone()).collect();

        let mut btree_map : BTreeMap<Value, Vec<u8>> = BTreeMap::new();

        for value in unique_values.iter() {
            let mut vec_len = length / 8;
            if length % 8 > 0 {
                vec_len += 1;
            }
            let vec: Vec<u8> = vec![0; vec_len];
            btree_map.insert(*value, vec);
        }

        let mut table_scan = TableScan::new(table.clone());
        table_scan.open();
        for (index, record) in table_scan.into_iter().enumerate() {
            let value = record.get(key_column_number).unwrap();

            for (_, vec) in btree_map.range_mut(..=value) {
                let page_index = index / 8;
                let bit_index = index % 8;

                vec[page_index] |= 1 << bit_index;
            }
        }

        Self {
            number_of_records: length,
            map: btree_map
        }
    }
}

impl BitmapIndex for RangeEncodedBitmapIndex {
    fn range_lookup(&self, start_key: Value, end_key: Value) -> IndexIterator {
        let (_, bit_map_start) = self.map.range(start_key..=end_key).next().unwrap();

        let mut range = self.map.range(end_key..);
        let Some((value, mut bit_map_end)) = range.next() else { panic!() };
        if *value == end_key {
            let Some((_, next_val)) = range.next() else { panic!() };
            bit_map_end = next_val;
        }

        let mut bitmap = bit_map_start.clone();

        for (byte, byte2) in bitmap.iter_mut().zip(bit_map_end.iter()) {
            *byte &= !byte2;
        }

        IndexIterator::new(bitmap)
    }
}
