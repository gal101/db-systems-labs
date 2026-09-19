use crate::iceberg::{Catalog, ColumnStats, FileStats, Manifest, MinMax};
use crate::storage::{DataFile, DataFileHeader, FileBasedStorage, FileHandle};
use crate::{DatabaseError, Record, TableChunk, TypeID, Value};
use std::collections::HashSet;
use crate::engine::SdmsIcebergEngine;


pub fn generate_column_info(chunk: &TableChunk) -> Vec<TypeID> {
    let mut column_info: Vec<TypeID> = Vec::new();
    for column in chunk {
        column_info.push(column[0].get_id());
    }
    column_info
}

impl SdmsIcebergEngine {
    pub fn new(catalog: Catalog, storage: FileBasedStorage) -> SdmsIcebergEngine {
        SdmsIcebergEngine {
            catalog,
            manifest: Manifest::default(),
            table_id: None,
            storage,
            changed_files: HashSet::new(),
        }
    }

    /// Starts a new table modification by setting the table_id member.
    /// Needs to be called before "insert/update/delete/delete_chunks".
    /// A table modification is concluded by calling "commit".
    pub fn start_table_modification(&mut self, table_id: usize) -> Result<(), DatabaseError> {
        if self.table_id.is_some() || !self.catalog.check_table_exists(table_id) {
            return Err(DatabaseError::EngineError);
        }
        self.table_id = Some(table_id);
        Ok(())
    }

    /// End a modification, creating a new version if files have changed.
    /// Stops the current modification by resetting the table_id.
    pub fn commit(&mut self) -> Result<(), DatabaseError> {
        if let Some(table_id) = self.table_id {
            let table_metadata = self.catalog.get_table_metadata(table_id);

           if !self.changed_files.is_empty() {
                // Moves self.manifest to table_metadata and replaces it with Manifest::default()
                table_metadata.add_version(std::mem::take(&mut self.manifest));
                self.changed_files = HashSet::new();
            }
            self.table_id = None;

            Ok(())
        } else {
            Err(DatabaseError::EngineError)
        }
    }

    /// We suggest to implement this helper function to calculate MinMax FileStats for a chunk.
    fn calculate_statistics(chunk: &TableChunk) -> FileStats {
        let mut vec = Vec::new();
        for column in chunk {
            let min = column.iter().min().unwrap();
            let max = column.iter().max().unwrap();

            vec.push(ColumnStats::MinMax(MinMax::new(min.clone(), max.clone())));
        }

        FileStats::new(vec)
    }

    /// Insert the chunks into the table by calculating their stats, writing them to a file each
    /// using the storage module, and adding them to the manifest.
    /// Return DatabaseError::EngineError if no table modification is ongoing.
    pub fn insert(&mut self, chunks: Vec<TableChunk>) -> Result<(), DatabaseError> {
        if self.table_id == None {
            return Err(DatabaseError::EngineError);
        }
        
        for chunk in chunks.iter() {
            let stats = Self::calculate_statistics(chunk); // get statistics for each chunk
            let column_info= generate_column_info(chunk); // get column typeids for each chunk
            let rows = chunk[0].len() as u64;
            let columns = chunk.len() as u64;

            let datafile = DataFile::new(DataFileHeader::new(rows, columns, column_info), chunk.clone()); // generate datafile
            let file_handle = self.storage.write_file(&datafile.to_bytes())?; // write datafile to disk

            self.manifest.add_file(file_handle, stats);
            self.changed_files.insert(file_handle);
        }
        Ok(())
    }

    /// Update the table by loading the changed chunks from their files, overwriting the rows with
    /// the records given in updates, writing the changed chunk into a new file, calculating new
    /// statistics, and recording the changes in the manifest.
    /// If a file handle is already contained in the changes of this manifest, do not change files
    /// or the manifest and return DatabaseError::EngineError.
    /// Return DatabaseError::EngineError if no table modification is ongoing.
    ///
    /// An update consists of a row ID (u32) and a Record containing the new data for this row.
    /// Multiple rows in one file can be updated by one call of this function (Vec<(u32, Record)>).
    /// Multiple files can be updated by one call of this function by calling it with a vector of
    /// FileHandles and corresponding updates
    pub fn update(
        &mut self,
        updates: Vec<(FileHandle, Vec<(u32, Record)>)>,
    ) -> Result<(), DatabaseError> {
        if self.table_id == None {
            return Err(DatabaseError::EngineError);
        }
        let mut set: HashSet<FileHandle> = HashSet::new();
        let files = self.catalog.get_table_metadata(self.table_id.unwrap()).files(None);
        for (file, _) in updates.iter() {
            if self.changed_files.contains(file) || !files.contains(file) { // checkin for invalid files in input
                return Err(DatabaseError::EngineError)
            }
            set.insert(*file);
        }
        
        if set.len() < updates.len() { // checking for duplicates in the input array
            return Err(DatabaseError::EngineError)
        }
        
        for(file_handle, changes) in updates.iter() {
            let mut file = self.storage.read_file(file_handle)?; // read each file into memory

            let mut datafile = DataFile::parse(&mut file)?;
            for (row_index, record) in changes.iter() {
                for (column, value) in datafile.data.iter_mut().zip(record.record.iter()) {
                    column[*row_index as usize] = value.clone(); // make changes
                }
            }

            self.changed_files.insert(*file_handle); // mark as changed file
            let new_file_handle = self.storage.write_file(&datafile.to_bytes())?; // write file to disk
            let stats = Self::calculate_statistics(&datafile.data); // calculate statistics
            self.manifest.add_file(new_file_handle, stats); // add new file to manifest
            self.manifest.delete_file(*file_handle); // delete old file from manifest
        }
        Ok(())
    }

    /// Update the table by loading the specified chunks from their files, deleting the rows
    /// specified in deletions, storing the changed chunks in new files, calculating new
    /// statistics, and recording the changes in the manifest.
    /// You can assume that the row indexes in deletions are in ascending order!
    /// If a file handle is already contained in the changes of this manifest, do not change files
    /// or the manifest and return DatabaseError::EngineError.
    /// Return DatabaseError::EngineError if no table modification is ongoing.
    ///
    /// A row to delete is represented as u32.
    /// Multiple rows in one file can be deleted by one call of this function (Vec<u32>).
    /// Rows in Multiple files can be deleted by one call of this function by calling it with a
    /// vector of FileHandles and corresponding deletions.
    pub fn delete(&mut self, deletions: Vec<(FileHandle, Vec<u32>)>) -> Result<(), DatabaseError> {
        if self.table_id == None {
            return Err(DatabaseError::EngineError);
        }
        let set: HashSet<FileHandle> = deletions.iter().map(|(file_handle, _)| file_handle.clone()).collect();        
        if set.len() < deletions.len() { // check for duplicates using hashset
            return Err(DatabaseError::EngineError)
        }
        let files = self.catalog.get_table_metadata(self.table_id.unwrap()).files(None);
        for (file, _) in deletions.iter() {
            if self.changed_files.contains(file) || !files.contains(file) { // check for invalid files
                return Err(DatabaseError::EngineError)
            }
        }
        
        for(file_handle, rows_to_delete) in deletions.iter() {
            let mut file = self.storage.read_file(file_handle)?; // read file from disk

            let mut datafile = DataFile::parse(&mut file)?; // get datafile

            let mut new_chunk = TableChunk::new(); 
            for column in datafile.data.iter_mut() {
                let new_column: Vec<Value> = column.iter()
                    .enumerate()
                    .filter(move |(index, _)| !rows_to_delete.contains(&(*index as u32))) // keep only the valid rows for each column
                    .map(|(_, value)| value.clone())
                    .collect();
                let length = new_column.len(); // new column length
                new_chunk.push(new_column);
                if length == 0 { // if length == 0 for any column we can stop, we don't need to build a new file
                    break;
                }
            }

            self.changed_files.insert(*file_handle); // mark changes

            //GENERATE NEW DATA FILE -> (because rows changed)
            let rows = new_chunk[0].len() as u64;
            let columns = new_chunk.len() as u64;
            if new_chunk[0].len() > 0 {
                let new_header = DataFileHeader::new(rows, columns, generate_column_info(&new_chunk));
                let new_datafile = DataFile::new(new_header, new_chunk);
                let new_file_handle = self.storage.write_file(&new_datafile.to_bytes())?;
                let stats = Self::calculate_statistics(&new_datafile.data);
                self.manifest.add_file(new_file_handle, stats);
            }

            self.manifest.delete_file(*file_handle);
        }
        Ok(())
    }

    /// Update the table by marking the FileHandles in deletions as deleted in the manifest.
    /// If a file handle is already contained in the changes of this manifest, do not change files
    /// or the manifest and return DatabaseError::EngineError.
    /// Return DatabaseError::EngineError if no table modification is ongoing.
    pub fn delete_chunks(&mut self, deletions: &[FileHandle]) -> Result<(), DatabaseError> {
        if self.table_id == None {
            return Err(DatabaseError::EngineError);
        }

        let set: HashSet<FileHandle> = deletions.iter().map(|file_handle| file_handle.clone()).collect();
        if set.len() < deletions.len() { // check for duplicates
            return Err(DatabaseError::EngineError)
        }
        
        let files = self.catalog.get_table_metadata(self.table_id.unwrap()).files(None);
        for file in deletions.iter() {
            if self.changed_files.contains(file) || !files.contains(file) { // check for invalid files
                return Err(DatabaseError::EngineError)
            }
        }

        for file in deletions.iter() {
            self.manifest.delete_file(*file);
            self.changed_files.insert(*file);
        }
        Ok(())
    }
}
