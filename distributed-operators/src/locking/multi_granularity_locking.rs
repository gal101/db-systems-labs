use std::path::Component::ParentDir;
use crate::locking::*;

/// The underlying locktree that will implement the MultiGranularityLocking trait
///
/// # Attention!
/// Do not rename this struct, it gets instantiated in the tests!
pub struct MultiGranularityLockTree {
    root: MGLNode,
    // Add more members here if necessary
}

impl MultiGranularityLockTree {
    /// Construct a multi-granularity lock tree with initialized struct members.
    /// The root node here is never part of the path in the get_state and lock functions.
    pub fn new(root: MGLNode) -> Self {
        Self { root }
    }
}

    fn can_grant(req: &LockRequest, node: &MGLNode, intention: bool) -> bool {
        //node is NL
        if node.locks.is_empty() {
            return true
        }

        //node is X
        if node.locks.len() == 1 {
            let Some((tid, lock)) = node.locks.iter().next() else { panic!() };
            if *lock == MGLLock::Write {
                return false
            }
        }

        //request is X
        if *req == LockRequest::Write && !intention {
            return  false
        }

        //request is "IS"
        if *req == LockRequest::Read && intention {
            return true
        }

        //request is S
        if *req == LockRequest::Read && !intention {
            //if node is already IX
            if node.locks.iter().any(|(_, lock)| *lock == MGLLock::IntentionWrite) {
                return false
            }

            return true
        }

        //->request is IX

        //if node is already S
        if node.locks.iter().any(|(_, lock)| *lock == MGLLock::Read) {
            return false
        }

        true
    }

    fn release_lock_recursive(tid: TID, node: &mut MGLNode) -> bool {
        let locks: Vec<_> = node.locks
            .iter()
            .filter(|(id, _)| *id == tid)
            .cloned()
            .collect();

        if locks.len() > 0 {
            for lock in locks {
                node.locks.remove(&lock);
            }
            for (_, child) in node.children.iter_mut() {
                release_lock_recursive(tid, child);
            }
            
            true
        } else {
            false
        }
    }

impl MultiGranularityLocking for MultiGranularityLockTree {
    /// See documentation in ./mod.rs, and check exercise and lecture slides.
    fn get_state(&mut self, path: Vec<&str>) -> &mut MGLNode {
        let mut current_node = &mut self.root;
        for string in path {
            current_node = current_node.children.get_mut(string).unwrap();
        }
        current_node
    }

    /// See documentation in ./mod.rs, and check exercise and lecture slides.
    fn lock(&mut self, tid: TID, path: Vec<&str>, req: LockRequest) -> Vec<MGLLock> {
        let mut vec: Vec<MGLLock> = Vec::new();
        let mut current_node = &self.root;
        for (index, string) in path.iter().enumerate() {
            current_node = current_node.children.get(*string).unwrap();
            if index == path.len() - 1 {
                break;
            }

            //all these nodes are Intention Write/Read
            if can_grant(&req, current_node, true) {
                let lock = if req == LockRequest::Write {
                    MGLLock::IntentionWrite
                } else {
                    MGLLock::IntentionRead
                };
                
                vec.push(lock);
            } else {
                vec.push(MGLLock::Denied);
                self.release(tid).expect("Cannot release when denied");
                return vec;
            }
        }

        //last node is Read or Write
        if can_grant(&req, current_node, false) {
            let lock = if req == LockRequest::Write {
                MGLLock::Write
            } else {
                MGLLock::Read
            };

            vec.push(lock);
        } else {
            vec.push(MGLLock::Denied);
            self.release(tid);
            return vec;
        }

        let mut current_node = &mut self.root;
        for (string, lock) in path.iter().zip(vec.iter()) {
            current_node = current_node.children.get_mut(*string).unwrap();
            current_node.locks.insert((tid, lock.clone()));
        }
        vec
    }

    /// See documentation in ./mod.rs, and check exercise and lecture slides.
    fn release(&mut self, tid: TID) -> Result<(), LockingError> {
        let current_node = &mut self.root;
        let mut has_locks = false;
        for (_, child_node) in current_node.children.iter_mut() {
            has_locks = has_locks || release_lock_recursive(tid, child_node);
        }
        if !has_locks {
            return Err(LockingError::LockNotHeld)
        }


        Ok(())
    }
}
