use crate::operator::{Operator, Record};
use std::cmp::Ordering;

/// An operator that emits its input in sorted order.
/// "cmp" is a compare function that defines the sort order.
pub struct Sort {
    child: Box<dyn Operator>,
    cmp: fn(&Record, &Record) -> Ordering,
    vec: Vec<Record>,
    is_open: bool
}

impl Sort {
    pub fn new(child: Box<dyn Operator>, cmp: fn(&Record, &Record) -> Ordering) -> Self {
        Sort{
            child,
            cmp,
            vec: Vec::new(),
            is_open: false
        }
    }
}

impl Operator for Sort {
    /// Open the child operator and sort its output into an intermediate data structure.
    fn open(&mut self) {
        self.is_open = true;
        self.child.open();
        while let Some(record) = self.child.next() {
            self.vec.push(record);
        }

        let cmp = self.cmp;
        self.vec.sort_by(|a, b| cmp(b, a));
    }

    /// Return the child output in sorted order, or None when done.
    fn next(&mut self) -> Option<Record> {
        if !self.is_open {
            panic!();
        }
        self.vec.pop()
    }

    fn close(&mut self) {
        self.child.close();
        self.is_open = false;
        self.vec.clear();
    }
}
