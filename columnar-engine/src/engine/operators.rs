use crate::Operator;
use crate::storage::{Columns, DataFile};
use crate::storage::{FileBasedStorage, FileHandle};
use crate::{TableChunk, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

pub struct ColumnTableScan {
    files: Vec<FileHandle>,
    col_project: Columns,
    storage: FileBasedStorage,
    open: bool,
    index: usize
}

impl ColumnTableScan {
    pub fn new(files: Vec<FileHandle>, col_project: Columns, storage: FileBasedStorage) -> Self {
        Self {
            files,
            col_project,
            storage,
            open: false,
            index: 0
        }
    }
}

impl Operator for ColumnTableScan {
    /// Initialise child operators and itself.
    fn open(&mut self) {
        self.open = true;
        self.index = 0;
    }

    /// Get the next chunk (part of the table), in this case the columns of one file.
    /// Returns [None] if there are no more files to read.
    /// Ensure that the TableChunk.len() is always the total number of columns in the table.
    fn next(&mut self) -> Option<TableChunk> {
        if !self.open {
            panic!("Operator not open")
        }

        //get the current file
        let current_file = self.files.get(self.index);
        self.index += 1;
        match current_file {
            None => None,
            Some(file_handle) => {
                //read the file data
                let mut file = self.storage.read_file(file_handle).unwrap();
                let data_file = DataFile::parse(&mut file).unwrap();
                let chunk = data_file.data;
                match &self.col_project {
                    Columns::All => { // just return the chunk
                        Some(chunk)
                    },
                    Columns::Selection(sel) => {
                        let new_chunk: Vec<Vec<Value>> = chunk.iter()
                            .enumerate()
                            .map(|(index, column)| {
                                if sel.contains(&index) {
                                    column.clone() // keep projected columns
                                } else {
                                    Vec::new() // replace unwanted columns with empty vector
                                }
                            })
                            .collect();
                        Some(new_chunk)
                    }
                }
            }
        }
    }
    // Cleans up child and itself.
    fn close(&mut self) {
        self.open = false;
        self.index = 0;
    }
}

/// An operator for conjunctive filters.
pub struct ColumnFilter {
    child: Box<dyn Operator>,
    filters: HashMap<usize, (Value, Value)>,
    open: bool
}

impl ColumnFilter {
    pub fn new(child: Box<dyn Operator>, filters: HashMap<usize, (Value, Value)>) -> Self {
        Self {
            child,
            filters,
            open: false
        }
    }
}

impl Operator for ColumnFilter {
    // Initialise child operators and itself.
    fn open(&mut self) {
        self.open = true;
        self.child.open();
    }

    /// Get the next chunk (part of the table), filtered down to respective rows.
    /// Returns None if there are no more chunks to read.
    /// Preserve the order of rows per chunk.
    fn next(&mut self) -> Option<TableChunk> {
        if !self.open {
            panic!();
        }

        // get chunk from child
        let chunk = self.child.next();
        match chunk {
            None => None,
            Some(mut chunk) => {
                let rows = chunk.iter()
                    .map(|column| column.len())
                    .max()
                    .unwrap_or_default(); // get rows of chunk as the maximum rows in the chunk -> because some columns might be empty

                let mut rows_to_keep: HashSet<usize> = (0..rows).collect(); // list of rows that meet the filter criteria
                for (column_index, (left, right)) in self.filters.iter() {
                    let column = &chunk[*column_index];
                    for (index, elem) in column.iter().enumerate() {
                        if !(elem >= left && elem <= right) {
                            rows_to_keep.remove(&index); // remove filtered rows
                        }
                    }
                }

                let rows_to_keep = &rows_to_keep;
                for column in chunk.iter_mut() {
                    *column = column.drain(..)
                        .enumerate()
                        .filter(move |(index, _)| {
                            rows_to_keep.contains(index) // actually change the columns to only keep filtered rows
                        })
                        .map(|(_, elem)| elem)
                        .collect();
                }
                Some(chunk)
            }
        }
    }

    /// Cleans up child and itself.
    fn close(&mut self) {
        self.open = false;
        self.child.close();
    }
}

pub type AggFunc = Box<dyn Fn(&Vec<Value>) -> Value>;

pub struct ColumnAggregate {
    child: Box<dyn Operator>,
    aggregates: BTreeMap<usize, AggFunc>,
    open: bool
}

impl ColumnAggregate {
    pub fn new(child: Box<dyn Operator>, aggregates: HashMap<usize, AggFunc>) -> Self {
        Self {
            child,
            aggregates: aggregates.into_iter().collect(),
            open: false
        }
    }
}

impl Operator for ColumnAggregate {
    /// Initialise child operators and itself.
    fn open(&mut self) {
        self.child.open();
        self.open = true;
    }

    /// Returns a chunk containing the projected and aggregated column values.
    /// Returns exactly one row.
    /// Preserve the order of aggregation columns.
    fn next(&mut self) -> Option<TableChunk> {
        if !self.open {
            panic!();
        }
        let mut chunk_option = self.child.next();
        if chunk_option == None {
            return None
        }
        let projected_cols = self.aggregates.len();
        let mut vec = vec![vec![]; projected_cols];

        while let Some(ref chunk) = chunk_option { // get all chunks from left child
            for (new_index, (column_index, function)) in self.aggregates.iter().enumerate() {
                let result = function(&chunk[*column_index]); // calculate aggregate result for every chunk
                vec[new_index].push(result); // add it to our list of aggregates
            }

            chunk_option = self.child.next();
        }

        for (new_index, (_, function)) in self.aggregates.iter().enumerate() {
            let result = function(&vec[new_index]); // aggregate all the results calculated for each column
            vec[new_index].clear();
            vec[new_index].push(result); // replace our list with the result -> we only get one row
        }
        Some(vec)
    }

    /// Cleans up child and itself.
    fn close(&mut self) {
        self.child.close();
        self.open = false;
    }
}

pub struct ColumnEqJoin {
    left_data: TableChunk, // all data from the left child combined in one chunk
    hash_map: HashMap<Value, usize>, // hashmap for left join values
    child0: Box<dyn Operator>,
    child1: Box<dyn Operator>,
    join_column_idxs: (usize, usize),
    open: bool
}

impl ColumnEqJoin {
    pub fn new(
        child0: Box<dyn Operator>,
        child1: Box<dyn Operator>,
        join_column_idxs: (usize, usize),
    ) -> Self {
        Self {
            left_data: Vec::new(),
            hash_map: HashMap::new(),
            child0,
            child1,
            join_column_idxs,
            open: false
        }
    }
}

impl Operator for ColumnEqJoin {
    /// Initialize child operators and itself.
    fn open(&mut self) {
        self.child0.open();
        self.child1.open();
        self.open = true;

        let mut chunk_option = self.child0.next();
        if chunk_option == None {
            panic!();
        }

        let Some(ref first_chunk) = chunk_option else { panic!() };
        let columns = first_chunk.len();
        self.left_data = vec![vec![]; columns];

        while let Some(ref chunk) = chunk_option {
            // get all chunks from left child
            for (chunk_column, data_column) in chunk.iter().zip(self.left_data.iter_mut()) {
                data_column.extend_from_slice(chunk_column); // combine all chunks in one big chunk
            }

            chunk_option = self.child0.next();
        }

        let join_column = &self.left_data[self.join_column_idxs.0];
        if !join_column.is_empty() {
            for(index, value) in join_column.iter().enumerate() {
                self.hash_map.insert(value.clone(), index); // generate hashmap for left values from the big chunk
            }
        }
    }

    /// Get the next chunk of child0 rows joined with child1 rows.
    /// Preserve the order of rows of each of child1's chunks (between chunks and inside chunks).
    /// Return None when no more chunks are available in the right input.
    fn next(&mut self) -> Option<TableChunk> {
        if !self.open {
            panic!();
        }
        let chunk = self.child1.next(); // get next chunk from right child
        match chunk {
            None => None,
            Some(chunk) => {
                let left_columns = self.left_data.len();

                let right_columns = chunk.len();
                let right_rows = chunk[self.join_column_idxs.1].len();

                let mut vec_result = vec![vec![]; left_columns + right_columns]; // allocating size for result vector

                for i in 0..right_rows {
                    let value = &chunk[self.join_column_idxs.1][i];
                    let found = self.hash_map.get(value); // getting row index from hashmap
                    if let Some(found_index) = found {
                        for (index, column) in self.left_data.iter().enumerate() {
                            if !column.is_empty() {
                                vec_result[index].push(column[*found_index].clone()); // add values from non-empty column for this row (left child)
                            }
                        }
                        for (index, column) in chunk.iter().enumerate() {
                            if !column.is_empty() {
                                vec_result[index + left_columns].push(column[i].clone()); // add values from non-empty column for this row (right child)
                            }
                        }
                    }
                }

                Some(vec_result)
            }
        }
    }

    /// Cleans up the child operators and itself.
    fn close(&mut self) {
        self.child1.close();
        self.child0.close();
        self.open = false;
        self.hash_map.clear();
        self.left_data.clear();
    }
}
