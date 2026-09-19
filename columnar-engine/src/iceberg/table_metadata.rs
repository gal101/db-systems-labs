use super::{Manifest, TableMetadata, version::Version};
use crate::storage::FileHandle;
use crate::{Schema, Value};
use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::ptr::hash;

impl TableMetadata {
    pub fn new(name: String, schema: Schema) -> Self {
        TableMetadata {
            name,
            schema,
            version: Version::default(),
            manifests: Vec::new(),
        }
    }

    /// Return all manifests (versions) until the input version as a slice. If version is None,
    /// return all manifests (the current version).
    pub fn snapshot(&self, version: Option<Version>) -> &[Manifest] {
        match version {
            None => {
                self.manifests.as_slice()
            },
            Some(version) => {
                let index = u64::from(version) as usize;
                if index < self.manifests.len() { // check for index out of bounds
                    &self.manifests.as_slice()[0..index]
                } else {
                    &self.manifests.as_slice()
                }
            }
        }
    }

    /// Return all file handles of files that belong to the table at the specified version.
    pub fn files(&self, version: Option<Version>) -> HashSet<FileHandle> {
        let snapshot = self.snapshot(version);
        let mut hash_set = HashSet::new();
        for manifest in snapshot {
            hash_set.extend(manifest.added());

            for file in manifest.deleted() {
                hash_set.remove(file); // remove deleted files
            }
        }

        hash_set
    }

    /// Return all file handles of files that belong to the table at the specified version and
    /// overlap with all predicates. Predicates are represented as the value to search for and the
    /// column ID as usize. Predicates are conjunctive. Thus, all predicates must match.
    pub fn contains(
        &self,
        predicates: Vec<(Value, usize)>,
        version: Option<Version>,
    ) -> HashSet<FileHandle> {
        let snapshot = self.snapshot(version);
        let mut hash_set = HashSet::new();
        let files = self.files(version);

        for manifest in snapshot {
            hash_set.extend(manifest.contains(predicates.clone())); // select all files that match
        }
        
        hash_set.retain(|file| files.contains(file)); // keep only files that aren't deleted

        hash_set
    }

    /// Return all file handles of files that belong to the table at the specified version and
    /// overlap with all predicates. Predicates are represented as the range to search for and the
    /// column ID. Predicates are conjunctive. Thus, all predicates must match.
    pub fn contains_range(
        &self,
        predicates: Vec<(RangeInclusive<Value>, usize)>,
        version: Option<Version>,
    ) -> HashSet<FileHandle> {
        let snapshot = self.snapshot(version);
        let mut hash_set = HashSet::new();
        let files = self.files(version);

        for manifest in snapshot {
            hash_set.extend(manifest.contains_range(predicates.clone())); // select all files that match
        }

        hash_set.retain(|file| files.contains(file)); // keep only files that aren't deleted

        hash_set
    }

    /// Add a manifest as a new version to the table and update the table version
    pub fn add_version(&mut self, manifest: Manifest) -> Version {
        self.manifests.push(manifest);
        self.version = self.version.successor();
        self.version
    }
}
