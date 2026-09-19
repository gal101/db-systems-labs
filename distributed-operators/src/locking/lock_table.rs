use crate::RowID;
use crate::locking::*;
use crate::locking::LockingError::LockNotHeld;

/// The underlying locktable that will implement the BasicLocking trait:
/// Do not rename this struct, it gets instantiated in the tests!
pub struct BasicLockTable {
    // Add the necessary members here
    hash_map: HashMap<RowID, BasicLockState>
}

impl BasicLockTable {
    /// Construct a locktable with initialized struct members:
    pub fn new() -> Self {
        Self {
            hash_map: HashMap::new()
        }
    }
}

    fn check_and_merge(vec: &mut Vec<(TID, LockRequest)>) {
        if vec.len() == 2 && vec[0].0 == vec[1].0 {
            vec.remove(0);
        }
    }

impl BasicLocking for BasicLockTable {
    /// See documentation in ./mod.rs, and check exercise and lecture slides.
    fn get_state(&self, object: RowID) -> Option<&BasicLockState> {
        self.hash_map.get(&object)
    }

    /// See documentation in ./mod.rs, and check exercise and lecture slides.
    fn lock(
        &mut self,
        tid: TID,
        object: RowID,
        req: LockRequest,
    ) -> Result<BasicLockResult, LockingError> {
        if self.hash_map.contains_key(&object) {
            let lock_state = self.hash_map.get_mut(&object).unwrap();
            match req {
                LockRequest::Read => {
                    if lock_state.readers.contains(&tid) || lock_state.writer.is_some_and(|id| id == tid) {
                        return Err(LockingError::LockAlreadyHeld)
                    }

                    if lock_state.writer == None {
                        lock_state.readers.insert(tid);
                        Ok(BasicLockResult::Granted)
                    } else {
                        lock_state.queue.push_back((tid, req));
                        Ok(BasicLockResult::Queued)
                    }
                },
                LockRequest::Write => {
                    if let Some(write_lock_id) = lock_state.writer {
                        return if write_lock_id == tid {
                            Err(LockingError::LockAlreadyHeld)
                        } else {
                            lock_state.queue.push_back((tid, req));
                            Ok(BasicLockResult::Queued)
                        }
                    }

                    if lock_state.readers.len() > 1 {
                        lock_state.queue.push_back((tid, req));
                        Ok(BasicLockResult::Queued)
                    } else if lock_state.readers.len() == 1 {
                        //if the tid has read lock but wants write lock
                        if lock_state.readers.get(&tid).is_some() {
                            lock_state.readers.clear();
                            lock_state.writer = Some(tid);
                            Ok(BasicLockResult::Granted)
                        } else {
                            lock_state.queue.push_back((tid, req));
                            Ok(BasicLockResult::Queued)
                        }
                    } else {
                        lock_state.writer = Some(tid);
                        Ok(BasicLockResult::Granted)
                    }
                }
            }
        } else {
            let mut lock_state = BasicLockState::default();
            match req {
                LockRequest::Read => {
                    lock_state.readers.insert(tid);
                },
                LockRequest::Write => {
                    lock_state.writer = Some(tid)
                }
            }
            self.hash_map.insert(object, lock_state);
            Ok(BasicLockResult::Granted)
        }
    }

    /// See documentation in ./mod.rs, and check exercise and lecture slides.
    fn release(
        &mut self,
        tid: TID,
        object: RowID,
    ) -> Result<Vec<(TID, LockRequest)>, LockingError> {
        let lock_state = self.hash_map.get_mut(&object).ok_or(LockNotHeld)?;

        if !lock_state.readers.contains(&tid) && (lock_state.writer.is_none() || lock_state.writer.is_some_and(|id| id != tid)) {
            return Err(LockingError::LockNotHeld)
        }

        if lock_state.readers.contains(&tid) {
            lock_state.readers.remove(&tid);
        }

        if lock_state.writer.is_some_and(|id| id == tid) {
            lock_state.writer = None;
        }

        let mut vec = Vec::new();
        //let mut queue = lock_state.queue.clone();
        loop {
            let lock_state = self.hash_map.get_mut(&object).unwrap();
            let next_object = lock_state.queue.front().cloned();
            match next_object {
                None => {
                    check_and_merge(&mut vec);
                    return Ok(vec)
                },
                Some((id, req)) => {
                    let result = self.lock(id, object, req.clone());

                    if result == Ok(BasicLockResult::Granted) {
                        vec.push((id, req.clone()));
                        self.hash_map.get_mut(&object).unwrap().queue.pop_front();
                    } else {
                        if result == Ok(BasicLockResult::Queued) {
                            self.hash_map.get_mut(&object).unwrap().queue.pop_back();
                        }
                        check_and_merge(&mut vec);
                        return Ok(vec)
                    }
                }
            }
        }
    }
}
