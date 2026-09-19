use crate::Operator;
use crate::Value;
use crate::index::index_iterator::IndexIterator;
use crate::index::shared::*;
use crate::index::{BitmapIndex, DecomposedBitmapIndex};
use crate::test_util::{DummyTable, TableScan};
use std::sync::Arc;



fn get_num_digits(number: usize, base: usize) -> usize {
    if number == 0 {
        return 1;
    }
    (number.ilog(base) + 1) as usize
}

fn decompose_number(mut number: usize, base: usize, digits: usize) -> Vec<usize> {
    let mut vec = Vec::with_capacity(digits);

    for _ in 0..digits {
        vec.push(number % base);
        number /= base;
    }

    vec
}

impl DecomposedBitmapIndex {
    /// Create a decomposed bitmap index in the specified base for the records in table,
    /// using the column key_column_number. The maximum value (i.e., the one with the longest
    /// representation in terms of digits) in that column is provided here as max_val.
    /// Values in the key column can be expected to always be Value::UInt.,
    /// base is always greater than 1.
    pub fn new(
        table: Arc<DummyTable>,
        key_column_number: usize,
        base: usize,
        max_val: usize,
    ) -> Self {
        let length = table.get_number_of_records();
        let digits = get_num_digits(max_val, base);
        let bitmap_len = (length + 7) / 8;
        let mut vec = vec![vec![vec![0u8; bitmap_len];base]; digits];

        let mut table_scan = TableScan::new(table.clone());
        table_scan.open();

        for (row_index, record) in table_scan.into_iter().enumerate() {
            let Value::UInt(number) = record.get(key_column_number).unwrap() else { panic!() };
            let mut value = *number;
            for i in 0..digits {
                let digit = value % base;
                vec[i][digit][row_index/8] |= 1 << (row_index % 8);

                value /= base;
            }
        }


        Self {
            number_of_records: length,
            total_digits: digits,
            vecs: vec
        }
    }

    /// This is only used in the benchmark! -> Not relevant for basic and advanced tests.
    /// The return value of this function is used as base for the DecomposedBitmapIndex in the
    /// benchmark.
    ///
    /// To achieve the best performance, you may try to optimize the return value or introduce a
    /// dynamic calculation based on the three input parameters.
    pub fn determine_optimal_base(
        _table: Arc<DummyTable>,
        _key_column_number: usize,
        _max_val: usize,
    ) -> usize {
        5
    }
}



impl BitmapIndex for DecomposedBitmapIndex {
    /// Same as for the creation of the decomposed bitmap index:
    /// start_key and end_key Values can be expected to always be usize.
    fn range_lookup(&self, start_key: Value, end_key: Value) -> IndexIterator {
        let base = self.vecs[0].len();
        let vec_len = (self.number_of_records + 7) / 8;
        let mut start_bitmap = vec![0u8; vec_len];
        let mut mask = vec![255u8; vec_len];

        let Value::UInt(start_number) = start_key else { panic!() };
        let mut start_decomposed = decompose_number(start_number, base, self.total_digits);
        //reverse to start from most significant digit
        start_decomposed.reverse();

        for (index, digit) in start_decomposed.iter().enumerate() {
            let real_digit_index = self.total_digits - index - 1;

            //adding rows greater than current digit
            for i in digit+1..base {
                let aux_bitmap = &self.vecs[real_digit_index][i];
                for ((a, b), c) in start_bitmap.iter_mut().zip(aux_bitmap.iter()).zip(mask.iter()) {
                    *a |= *b & *c;
                }
            }
            //adding rows equal to current digit to the mask
            let aux_bitmap = &self.vecs[real_digit_index][*digit];
            for (a, b) in mask.iter_mut().zip(aux_bitmap.iter()) {
                *a &= *b;
            }
        }

        //adding rows equal to start key
        for (a, b) in start_bitmap.iter_mut().zip(mask.iter()) {
            *a |= *b;
        }

        let Value::UInt(end_number) = end_key else { panic!() };
        let mut end_decomposed = decompose_number(end_number, base, self.total_digits);
        //reverse to start from most significant digit
        end_decomposed.reverse();

        let mut end_bitmap = vec![0u8; vec_len];
        let mut mask = vec![255u8; vec_len];

        for (index, digit) in end_decomposed.iter().enumerate() {
            let real_digit_index = self.total_digits - index - 1;

            //adding rows smaller than current digit
            for i in 0..*digit {
                let aux_bitmap = &self.vecs[real_digit_index][i];
                for ((a, b), c) in end_bitmap.iter_mut().zip(aux_bitmap.iter()).zip(mask.iter()) {
                    *a |= *b & *c;
                }
            }
            //adding rows equal to current digit to the mask
            let aux_bitmap = &self.vecs[real_digit_index][*digit];
            for (a, b) in mask.iter_mut().zip(aux_bitmap.iter()) {
                *a &= *b;
            }
        }

        //adding rows equal to end key
        for (a, b) in end_bitmap.iter_mut().zip(mask.iter()) {
            *a |= *b;
        }


        //combining start and end bitmaps
        for (a, b) in start_bitmap.iter_mut().zip(end_bitmap.iter()) {
            *a &= b;
        }
        
        IndexIterator::new(start_bitmap)
    }
}

