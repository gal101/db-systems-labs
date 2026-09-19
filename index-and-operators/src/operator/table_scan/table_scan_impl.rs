use std::{cell::RefCell, rc::Rc};

use crate::operator::{BufferPool, Operator, Record, Table};
use crate::{PageID, SlotID};

/// A TableScan that returns all records of all pages of the respective table.
/// Has access to a BufferPool to get data pages.
pub struct TableScan {
    table: Rc<dyn Table>,
    buffer: Rc<RefCell<dyn BufferPool>>,
    is_open: bool,
    page_index: usize,
    current_page_id: PageID,
    index_on_page: usize
}

impl TableScan {
    pub fn new(table: Rc<dyn Table>, buffer: Rc<RefCell<dyn BufferPool>>) -> Self {
        TableScan{
            table,
            buffer,
            is_open: false,
            page_index: 0,
            current_page_id: PageID(0),
            index_on_page: 0
        }
    }
}

impl Operator for TableScan {
    fn open(&mut self) {
        self.is_open = true;
        self.page_index = 0;
        self.current_page_id = self.table.page_list()[self.page_index];
        self.index_on_page = 0;
    }

    /// Emit the next record in this table, taken from the tables' pages.
    /// When all records have been emitted, emit None.
    fn next(&mut self) -> Option<Record> {
        if !self.is_open {
            panic!();
        }
        let page = self.buffer.borrow_mut().pin(self.current_page_id);

        if self.index_on_page < page.length {

            let record = page.records[self.index_on_page].clone();
            self.buffer.borrow_mut().unpin(page);
            self.index_on_page += 1;
            Some(record)

        } else {

            self.buffer.borrow_mut().unpin(page);
            self.page_index += 1;
            self.index_on_page = 0;

            if self.page_index < self.table.page_list().len() {

                self.current_page_id = self.table.page_list()[self.page_index];
                let page = self.buffer.borrow_mut().pin(self.current_page_id);

                let record = page.records[self.index_on_page].clone();
                self.buffer.borrow_mut().unpin(page);
                self.index_on_page += 1;
                Some(record)
            } else {
                None
            }
        }
    }

    fn close(&mut self) {
        self.is_open = false;
        self.index_on_page = 0;
        self.page_index = 0;
    }
}
