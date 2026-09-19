use std::collections::VecDeque;
use crate::RowID;

/// An iterator that translates a bitmap into increasing RowIDs (see description of bitmaps in file "src/index/mod.rs")
pub struct IndexIterator {
    // add necessary members here
    iter: Box<dyn Iterator<Item = RowID>>
}

impl IndexIterator {
    pub fn new(bitmap: Vec<u8>) -> Self {
        //creates an iterator of RowIDs over the bits set to 1
        let iter = bitmap.into_iter()
            .enumerate()
            .flat_map(|(page_idx, page)| {
                //go through all the bits of the page (page = an u8 in the Vec)
                (0..8).filter_map(move |bit_idx| {
                    if (page >> bit_idx) & 1 == 1 {
                        //get the correct RowID
                        let index = page_idx * 8 + bit_idx;
                        Some(RowID(index))
                    } else {
                        None
                    }
                })
            });

        Self {
            //using Box to simplify
            iter: Box::new(iter)
        }
    }
}

impl Iterator for IndexIterator {
    type Item = RowID;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }
}
