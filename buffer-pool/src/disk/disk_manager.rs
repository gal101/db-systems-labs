use std::fmt::Error;
use crate::disk::*;
use crate::{PAGE_SIZE, PageID};
use std::{
    collections::VecDeque,
    fs::OpenOptions,
    io::{self, Read, Seek, Write},
};
use std::io::SeekFrom;

impl DiskManager {
    /// Create a DiskManager to manage a database file on disk
    ///
    /// Always start with an empty file. Empty the file if it already exists.
    /// `next_free` should start at `PageID(1)`
    ///
    /// # Errors
    ///
    /// Will return [`io::Error`] if opening `filename` returns an error.
    pub fn new(filename: &str) -> Result<Self, io::Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(filename)?;

        Ok(DiskManager {
            file,
            next_free: PageID(1),
            free_list: VecDeque::new()
        })
    }

    /// Get a PageID for a new page, either from the `free_list` or using `next_free`.
    pub fn allocate(&mut self) -> PageID {
        if !self.free_list.is_empty() {
            self.free_list.pop_front().unwrap()
        } else {
            match self.next_free {
                PageID(x) => { self.next_free = PageID(x + 1); PageID(x) }
            }
        }
    }

    /// Mark the given page id as free
    ///
    /// This allows reusing the page id for new pages.
    ///
    /// # Errors
    /// Returns [`DiskManagerError::InvalidPageID`] if `page_id` is not in the
    /// interval of allocated pages or if the page is already on the free list.
    pub fn free(&mut self, page_id: PageID) -> Result<(), DiskManagerError> {
        if self.free_list.contains(&page_id) || page_id.0 >= self.next_free.0 {
            Err(DiskManagerError::InvalidPageID(page_id))
        } else {
            self.free_list.push_back(page_id);
            Ok(())
        }
    }

    /// Reads a page from the database file on disk
    ///
    /// PageID serves as an offset to the position of the page in the file.
    ///
    /// # Errors
    /// - Returns [`DiskManagerError::InvalidPageID`] if `page_id` is not in the
    ///   interval of allocated pages or if the page is on the free list.
    /// - Return [`DiskManagerError::IOError`] if file operations return an [`io::Error`].
    pub fn read(&mut self, page_id: PageID, buf: &mut RawPage) -> Result<(), DiskManagerError> {
        if self.free_list.contains(&page_id) || page_id.0 >= self.next_free.0 {
            Err(DiskManagerError::InvalidPageID(page_id))
        } else {
            self.file.seek(SeekFrom::Start(page_id.0 as u64 * PAGE_SIZE as u64))?;
            self.file.read(buf)?;
            Ok(())
        }
    }

    /// Writes page to the database file on disk
    ///
    /// PageID serves as an offset to the position of the page in the file.
    /// Ensure all data is written to disk by calling fsync.
    ///
    /// # Errors
    /// - Returns [`DiskManagerError::InvalidPageID`] if `page_id` is not in the
    ///   interval of allocated pages or if the page is on the free list.
    /// - Return [`DiskManagerError::IOError`] if file operations return an [`io::Error`].
    pub fn write(&mut self, page_id: PageID, buf: &RawPage) -> Result<(), DiskManagerError> {
        if self.free_list.contains(&page_id) || page_id.0 >= self.next_free.0 {
            Err(DiskManagerError::InvalidPageID(page_id))
        } else {
            self.file.seek(SeekFrom::Start(page_id.0 as u64 * PAGE_SIZE as u64))?;
            self.file.write(buf)?;
            Ok(())
        }
    }
}
