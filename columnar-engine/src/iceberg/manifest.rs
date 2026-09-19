use std::ops::RangeInclusive;

use super::{FileStats, Manifest};
use crate::Value;
use crate::storage::FileHandle;

impl Manifest {
    pub fn new(added: Vec<FileHandle>, deleted: Vec<FileHandle>, stats: Vec<FileStats>) -> Self {
        Manifest {
            added,
            deleted,
            stats,
        }
    }

    pub fn add_file(&mut self, file: FileHandle, stats: FileStats) {
        self.added.push(file);
        self.stats.push(stats);
    }

    pub fn delete_file(&mut self, file: FileHandle) {
        self.deleted.push(file);
    }

    pub fn added(&self) -> &[FileHandle] {
        self.added.as_slice()
    }

    pub fn deleted(&self) -> &[FileHandle] {
        self.deleted.as_slice()
    }

    /// Return an iterator over all file handles of files added in this manifest that have overlap
    /// with all the equality predicates. Predicates are represented as the value to search for and
    /// the column ID as usize. Predicates are conjunctive. Thus, all predicates must match.
    #[allow(redundant_semicolons)]
    pub fn contains(&self, predicates: Vec<(Value, usize)>) -> impl Iterator<Item = &FileHandle> {
        self.added.iter()
            .zip(self.stats.iter())
            .filter(move |(_, file_stats)| file_stats.contains(predicates.clone())) // keep only files that meet the predicates
            .map(|(file_handle, _)| file_handle) // keep only the handle
    }

    /// Return an iterator over all file handles of files added in this manifest that have overlap
    /// with all the range predicates. Predicates are represented as the range to search for and the
    /// column ID. Predicates are conjunctive. Thus, all predicates must match.
    #[allow(redundant_semicolons)]
    pub fn contains_range(
        &self,
        predicates: Vec<(RangeInclusive<Value>, usize)>,
    ) -> impl Iterator<Item = &FileHandle> {
        self.added.iter()
            .zip(self.stats.iter())
            .filter(move |(_, file_stats)| file_stats.contains_range(predicates.clone()))// the same as above
            .map(|(file_handle, _)| file_handle)
    }
}
