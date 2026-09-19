use std::mem::replace;
use crate::index::*;
use crate::{PageID, RecordID};
use crate::index::Page::INNER;

impl UniqueBPlusTree {
    /// Creates the initial B+-Tree where the root node is a leaf. You should leave this as it is.
    ///
    /// # Arguments
    ///
    /// * `buffer`: The vector to store the pages in. The expectation here is that the vector is
    ///   empty, and this function adds the first page to it.
    ///
    /// returns: UniqueBPlusTree
    pub fn new(buffer: &mut Vec<Page>) -> Self {
        let tree = Self {
            root_page: PageID(0),
        };
        let root_page = Page::default();
        buffer.push(root_page);
        tree
    }

    /// Searches for the entry with the exact corresponding key and returns its RecordID
    /// or None if the key is not found in the tree.
    ///
    /// # Arguments
    ///
    /// * `buffer`: The buffer manager containing the b-tree's pages.
    /// * `key`: The key to search for.
    ///
    /// returns: `Option<RecordID>` Some(RecordID) if found and None otherwise.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut buffer: Vec<Page> = Vec::new();
    /// let mut tree = UniqueBPlusTree::new(&mut buffer);
    ///
    /// tree.insert(&mut buffer, 69, RecordID::new(69u32.into(), 0.into()));
    /// let result = tree.lookup(&buffer, 69); // Some(RecordID(69, 0))
    /// ```
    pub fn lookup(&self, buffer: &Vec<Page>, key: i32) -> Option<RecordID> {
        let PageID(root_id) = self.root_page;
        if root_id as usize >= buffer.len() {
            return None
        }

        let leaf_page = self.lookup_leaf(self.root_page, buffer, key);
        let (index, found) = self.optimized_bsearch_leaf(leaf_page, key);

        if found {
            let record = leaf_page.records[index].value;
            Some(record)
        } else {
            None
        }
    }


    fn lookup_leaf<'a>(&self, page_id: PageID, buffer: &'a Vec<Page>, key: i32) -> &'a LeafPage {
        let page = buffer.get(page_id.0 as usize).unwrap();
        match page {
            Page::INNER(inner_page) => {
                let index = self.optimized_bsearch_inner(inner_page, key);
                let record = inner_page.records[index];
                let next_page_id = record.value.0;
                self.lookup_leaf(PageID(next_page_id), buffer, key)
            },
            Page::LEAF(leaf_page) => {
                leaf_page
            }
        }
    }

    /// Searches for all keys in the specified min/max range (both inclusive) and returns a vector
    /// of all corresponding RecordIDs.
    ///
    /// # Arguments
    ///
    /// * `buffer`: The buffer manager containing the b-tree's pages.
    /// * `range`: The range to search for.
    ///
    /// returns: Vec<RecordID, Global>
    ///
    /// # Examples
    ///
    /// ```
    /// let mut buffer: Vec<Page> = Vec::new();
    /// let mut tree = UniqueBPlusTree::new(&mut buffer);
    ///
    /// tree.insert(&mut buffer, 69, RecordID::new(69u32.into(), 0.into()));
    /// let result = tree.range_lookup(&buffer, (0, 100)); // Vec([RecordID(69, 0)])
    /// ```
    pub fn range_lookup(&self, buffer: &Vec<Page>, range: (i32, i32)) -> Vec<RecordID> {
        let PageID(root_id) = self.root_page;
        if root_id as usize >= buffer.len() {
            return Vec::new()
        }

        let (key_start, key_end) = range;

        let mut leaf_page = self.lookup_leaf(self.root_page, buffer, key_start);

        let mut vec = Vec::new();

        let (mut start_pos, _) = self.optimized_bsearch_leaf(leaf_page, key_start);

        'main: loop {
            let slice = &leaf_page.records[start_pos..leaf_page.length];
            for record in slice.iter() {
                if record.key > key_end {
                    break 'main;
                }
                
                if record.key >= key_start {
                    vec.push(record.value);
                }
            }
            start_pos = 0;

            let next_page_id = leaf_page.header.right;
            match next_page_id {
                None => break 'main,
                Some(page_id) => {
                    let Page::LEAF(next_page) = buffer.get(page_id.0 as usize).unwrap() else { panic!("neighbor not leaf") };
                    leaf_page = next_page;
                }
            }
        }

        vec
    }

    /// Insert a tuple with RecordID rid and key key into the BPlusTree. Overwrite the existing
    /// RecordID if the key already exists.
    ///
    /// # Arguments
    ///
    /// * `buffer`: The buffer manager containing the b-tree's pages.
    /// * `key`: The key to insert/update.
    /// * `rid`: The RecordID to insert.
    ///
    /// returns: ()
    ///
    /// # Examples
    ///
    /// ```
    /// let mut buffer: Vec<Page> = Vec::new();
    /// let mut tree = UniqueBPlusTree::new(&mut buffer);
    ///
    /// tree.insert(&mut buffer, 69, RecordID::new(69u32.into(), 0.into()));
    /// ```
    ///

    pub fn insert(&mut self, buffer: &mut Vec<Page>, key: i32, rid: RecordID) {
        if let Some((key, new_child_id)) = self.recursive_insert(self.root_page, buffer, key, rid) {
            let mut new_root = InnerPage::default();
            new_root.length = 2;
            new_root.id.0 = buffer.len() as u32;
            new_root.update(BTreeRecord{key, value: self.root_page}, 0).expect("error updating new root");
            new_root.records[1].value = new_child_id;

            self.root_page = new_root.id;
            let new_page = INNER(new_root);
            buffer.push(new_page);
        }
    }
    
    fn optimized_bsearch_inner(&self, inner_page: &InnerPage, key: i32) -> usize {
        let records = &inner_page.records[0..inner_page.length - 1];
        records.partition_point(|record| record.key <= key)
    }

    //returns a pair of (index, found)
    fn optimized_bsearch_leaf(&self, leaf_page: &LeafPage, key: i32) -> (usize, bool) {
        let records = &leaf_page.records[0..leaf_page.length];
        let res = records.binary_search_by_key(&key, |record| record.key);

        match res {
            Ok(idx) => {
                (idx, true)
            },
            Err(idx) => {
                (idx, false)
            }
        }
    }

    fn add_to_leaf(&self, rid: RecordID, key: i32, leaf_page: &mut LeafPage) {
        let (index, found) = self.optimized_bsearch_leaf(leaf_page, key);
        if found {
            leaf_page.records[index].value = rid;
        } else {
            let record = BTreeRecord {
                key,
                value: rid
            };
            leaf_page.insert(BTreeRecord { key, value: rid }, index).expect("insert error");
        }
    }

    fn split_inner(&mut self, buffer: &mut Vec<Page>, page_id: PageID, new_child: (i32, PageID)) -> (i32, PageID) {
        let mut new_inner = InnerPage::default();
        let new_id = PageID(buffer.len() as u32);
        let (key, value) = new_child;

        new_inner.id = new_id;
        let Page::INNER(old_page) = &mut buffer[page_id.0 as usize] else { panic!() };

        let m = old_page.length / 2;
        let key_to_promote = old_page.records[m].key;

        let start_index = m + 1;
        let new_len = old_page.length - start_index;

        new_inner.records[0..new_len].clone_from_slice(&old_page.records[start_index..old_page.length]);
        new_inner.length = new_len;

        old_page.length = m + 1;

        if key < key_to_promote {
            let index = self.optimized_bsearch_inner(old_page, key);
            let old_val = old_page.records[index].value;

            old_page.insert(BTreeRecord{key, value: old_val}, index).expect("insert in inner split");
            old_page.records[index + 1].value = value;
        } else {
            let index = self.optimized_bsearch_inner(&new_inner, key);
            let old_val = new_inner.records[index].value;
            new_inner.insert(BTreeRecord{key, value: old_val}, index).expect("insert in inner split");
            new_inner.records[index + 1].value = value;
        }

        buffer.push(Page::INNER(new_inner));

        (key_to_promote, new_id)
    }

    fn split_leaf(& self, buffer: &mut Vec<Page>, page_id: PageID, key: i32, rid: RecordID, insert_idx: usize) -> (i32, PageID) {
        let mut new_leaf = LeafPage::default();
        let new_page_id = PageID(buffer.len() as u32);
        let Page::LEAF(old_page) = &mut buffer[page_id.0 as usize] else { panic!() };

        let len = old_page.length;
        let split_idx = len / 2;
        new_leaf.id = new_page_id;
        new_leaf.length = split_idx;
        new_leaf.header.left = Some(page_id);
        new_leaf.header.right = old_page.header.right;

        old_page.length = split_idx;
        old_page.header.right = Some(new_leaf.id);
        
        if insert_idx < split_idx {
            new_leaf.records[0..split_idx].clone_from_slice(&old_page.records[split_idx..len]);
            old_page.insert(BTreeRecord { key, value: rid }, insert_idx).expect("insert error");
        } else {
            let first_len = insert_idx - split_idx;
            new_leaf.records[0..first_len].clone_from_slice(&old_page.records[split_idx..insert_idx]);
            new_leaf.records[first_len] = BTreeRecord { key, value: rid};
            new_leaf.records[first_len + 1..split_idx + 1].clone_from_slice(&old_page.records[insert_idx..len]);
            new_leaf.length += 1;
        }

        let comp_key = new_leaf.records[0].key;
        buffer.push(Page::LEAF(new_leaf));
        (comp_key, new_page_id)
    }

    fn recursive_insert(&mut self, page_id: PageID, buffer: &mut Vec<Page>, key: i32, rid: RecordID) -> Option<(i32, PageID)> {
        let page = buffer.get_mut(page_id.0 as usize).unwrap();
        match page {
            Page::INNER(inner_page) => {
                let index = self.optimized_bsearch_inner(inner_page, key);
                let next_page_id = inner_page.records[index].value;
                let result = self.recursive_insert(next_page_id, buffer, key, rid);
                match result {
                    None => None,
                    Some(new_data@(key_to_add, new_child_id)) => {
                        let INNER(inner_page) = buffer.get_mut(page_id.0 as usize).unwrap() else { panic!("Error getting page from buffer") };
                        if inner_page.length == InnerPage::CAPACITY {
                            Some(self.split_inner(buffer, page_id, new_data))
                        } else {
                            let index = self.optimized_bsearch_inner(inner_page, key_to_add);
                            let old_id = inner_page.records[index].value;
                            let old_key = inner_page.records[index].key;

                            inner_page.insert(BTreeRecord { key: key_to_add, value: old_id}, index).expect("error inserting in inner node");
                            inner_page.update(BTreeRecord{ key: old_key, value: new_child_id}, index + 1).expect("error updating");

                            None
                        }
                    }
                }
            },
            Page::LEAF(leaf_page) => {
                if leaf_page.length == LeafPage::CAPACITY {
                    let (index, found) = self.optimized_bsearch_leaf(leaf_page, key);
                    if found {
                        leaf_page.update(BTreeRecord{key, value: rid}, index).expect("Update leaf failed");
                        None
                    } else {
                        Some(self.split_leaf(buffer, page_id, key, rid, index))
                    }
                    
                } else {
                    self.add_to_leaf(rid, key, leaf_page);
                    None
                }
            }
        }
    }
}
