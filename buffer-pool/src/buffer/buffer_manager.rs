use std::{cell::RefCell, collections::HashMap, rc::Rc};
use std::collections::VecDeque;
use std::ops::{Index, IndexMut};
use crate::buffer::frame_pool::FramePool;
use crate::buffer::*;
use crate::{BUFFER_POOL_SIZE, FrameID, PageID};
/// Meta-data stored for a [`BufferManager`] *Frame*.
///
/// You are supposed to add information necessary to implement the replacement strategies
/// to this struct.
#[derive(Default)]
#[derive(Clone)]
pub struct FrameDescriptor {
    /// [PageID] of the page stored in frame.
    pub page_id: PageID,
    /// Number of times this page is currently pinned.
    pub pin_count: u16,
    /// true if and only if the page in [BufferManager] differs from the page on disk. Written by
    /// [BufferManager::unpin].
    pub dirty: bool,
    // Add new members here, but do not remove the members above.
    // The clock reference bit
    pub clock_bit: bool,
    // FrameId of the corresponding FrameDescriptor
    pub frame_id: FrameID,
    pub time: u64
}

/// Abstracts storage from higher database operations and caches pages from persistent storage.
///
/// Caches Pages from the `DiskManager` in a pool of [`BUFFER_POOL_SIZE`] frames. To lookup pages
/// that are loaded in the buffer manager, the [`BufferManager::page_table`] maps [`PageID`]s to
/// frames.
///
/// The metadata for each frame is stored in a [`FrameDescriptor`] instance. This is mainly used
/// for the [`ReplacementStrategy`] to evict pages, but also to store the dirty bit and the pin
/// count. (See documentation of [`FrameDescriptor`])
///
/// [`BufferManager`] is a generic type over a [`DiskManager`] that is used to access the disk and a
/// [`ReplacementStrategy`] that is used to determine the next page to evict when the buffer is
/// full.
///
/// # Important!
/// PageIDs for this BufferManager start with `PageID(1)`! `PageID(0)` is reserved to indicate
/// that no page was evicted yet!
///
/// The [last_evict] field is used in the advanced tests. It must be initialized to `PageID(0)` and
/// kept up to date after every eviction.
pub struct BufferManager<
    DiskManager: DiskManagerTrait,
    ReplacementStrategy: ReplacementStrategyTrait,
> {
    /// Access to underlying storage.
    /// The [`Rc`]<[`RefCell<>`]> is required in the advanced tests to use the same disk manager for
    /// two buffer managers.
    disk_manager: Rc<RefCell<DiskManager>>,
    /// Strategy to evict pages to load a new page. Must implement [`ReplacementStrategyTrait`].
    replacement_strat: ReplacementStrategy,
    /// Number of occupied frames.
    buffer_count: usize,
    /// Lookup table that contains the [`FrameID`] for every [`PageID`] that is currently loaded.
    pub page_table: HashMap<PageID, FrameID>,
    /// Metadata for each frame. See [`FrameDescriptor`] for more information on the metadata. See
     /// [`FramePool`] for documentation on how to use the wrapper.
    pub frame_descriptors: FramePool<FrameDescriptor>,
    /// Pool of cached pages. See [`FramePool`] for documentation on how to use the wrapper.
    pub pool: FramePool<MaterializedPage>,
    /// Used for advanced tests. **!! Must be initialized with PageID(0) AND kept up to date !!**
    #[allow(unused)]
    pub last_evict: PageID,
    // Add new members here, but do not remove the ones above.
}

/// Struct that can hold state required for LRU replacement.
/// You can also store state for the replacement strategy inside the [FrameDescriptor]s.
#[derive(Default)]
pub struct LRUReplacementStrategy {
    // you can add members here
    global_time: u64
}

/// Implement the LRU replacement strategy in [LRUReplacementStrategy::replace].
/// You can use state stored in both [LRUReplacementStrategy] and [FrameDescriptor]s.
///
/// Additionally, implement the updates to your [FrameDescriptor] in
/// [LRUReplacementStrategy::on_pin] that are required by LRU replacement.
///
/// # Important!
/// The replace function must be able to deal with different sizes of the [FramePool] than
/// [BUFFER_POOL_SIZE]. Use [FramePool::len] to get the size of the [FramePool].
impl ReplacementStrategyTrait for LRUReplacementStrategy {
    fn replace(
        &mut self,
        fds: &mut FramePool<FrameDescriptor>,
    ) -> Result<PageID, BufferManagerError> {
        let mut page_id: Option<PageID> = None;
        let mut min_time = self.global_time;
        for fd in fds.iter() {
            if fd.pin_count == 0 && fd.time < min_time {
                page_id = Some(fd.page_id);
                min_time = fd.time;
            }
        }

        match page_id {
            Some(page) => Ok(page),
            None => Err(BufferManagerError::AllPagesPinned)
        }
    }

    fn on_pin(&mut self, frame_descriptor: &mut FrameDescriptor) {
        frame_descriptor.time = self.global_time;
        self.global_time += 1;
    }
}

/// Struct that can hold state required for CLOCK replacement.
/// You can also store state for the replacement strategy inside the [FrameDescriptor]s.
#[derive(Default)]
pub struct ClockReplacementStrategy {
    clock_hand: FrameID
}

/// Implement the CLOCK replacement strategy in [ClockReplacementStrategy::replace].
/// You can use state stored in both [ClockReplacementStrategy] and [FrameDescriptor]s.
///
/// Additionally, implement the updates to your [FrameDescriptor] in
/// [LRUReplacementStrategy::on_pin] that are required by CLOCK replacement.
///
/// # Important!
/// This function must be able to deal with different sizes of the [FramePool] than
/// [BUFFER_POOL_SIZE]. Use [FramePool::len] to get the size of the [FramePool].
impl ReplacementStrategyTrait for ClockReplacementStrategy {
    fn replace(
        &mut self,
        fds: &mut FramePool<FrameDescriptor>,
    ) -> Result<PageID, BufferManagerError> {
        let mut i = 0;
        let len = fds.len();
        let FrameID(start ) = self.clock_hand;
        while i < 2 * len {
            let current_pos = (start + i) % len;
            let fd = fds.index_mut(FrameID(current_pos));
            if fd.pin_count == 0 {
                if fd.clock_bit == true {
                    fd.clock_bit = false;
                } else {
                    self.clock_hand = FrameID((current_pos + 1) % len);
                    return Ok(fd.page_id)
                }
            }

            i += 1;
        }

        Err(BufferManagerError::AllPagesPinned)
    }

    fn on_pin(&mut self, frame_descriptor: &mut FrameDescriptor) {
        frame_descriptor.clock_bit = true;
    }
}

impl<DiskManager: DiskManagerTrait, ReplacementStrategy: ReplacementStrategyTrait>
    BufferManager<DiskManager, ReplacementStrategy>
{
    /// Instantiates a new [`BufferManager`] instance, with [`ReplacementStrategy`]
    /// `replacement_strat` and [`DiskManager`] `disk_manager`.
    ///
    /// Initialize the buffer manager with its underlying resources here. Use [BUFFER_POOL_SIZE] as
    /// the size of `frame_descriptors` and `pool`.
    pub fn new(
        disk_manager: Rc<RefCell<DiskManager>>,
        replacement_strat: ReplacementStrategy,
    ) -> Self {
        let page_vec = vec![MaterializedPage::default(); BUFFER_POOL_SIZE];
        let page_pool = FramePool::new(page_vec.into_boxed_slice());

        let frame_desc_vec = vec![FrameDescriptor::default(); BUFFER_POOL_SIZE];
        let frame_pool = FramePool::new(frame_desc_vec.into_boxed_slice());

        BufferManager{
            disk_manager,
            replacement_strat,
            buffer_count: 0,
            page_table: HashMap::new(),
            frame_descriptors: frame_pool,
            pool: page_pool,
            last_evict: PageID(0)
        }
    }
}

impl<DiskManager: DiskManagerTrait, ReplacementStrategy: ReplacementStrategyTrait>
    BufferManagerTrait for BufferManager<DiskManager, ReplacementStrategy>
{
    /// Pins page *pid* and returns a mutable reference to it.
    ///
    /// If page *pid* is already loaded in the buffer manager, the function returns it.
    /// If not present, it loads the page in the buffer manager.
    /// In case no space is available in the buffer manager, one of the currently loaded pages
    /// must be evicted (replacement strategy decides how).
    /// The frame descriptor and the page table must be updated accordingly.
    ///
    /// # Safety
    /// For every [`BufferManager::pin`] call for a page ID, there must be a corresponding
    /// [`BufferManager::unpin`] call with the **same** page ID.
    ///
    /// # Error
    /// - Propagates errors from [`DiskManager::write`] and [`DiskManager::read`].
    /// - If all pages are pinned at least once, [`BufferManagerError::AllPagesPinned`] is returned.
    fn pin(&mut self, pid: PageID) -> Result<&mut MaterializedPage, BufferManagerError> {
        let frame;
        let frame_desc;

        if !self.page_table.contains_key(&pid) {
            //IF PAGE ID NOT IN BUFFER
            if self.buffer_count < BUFFER_POOL_SIZE {
                //IF BUFFER IS NOT FULL

                //Find the first empty frame
                let index = self.buffer_count;

                frame = FrameID(index);
                self.buffer_count += 1;
            } else {
                //IF BUFFER IS FULL
                //get the page_id to replace and the frame holding that page
                let page_id_to_replace = self.replacement_strat.replace(&mut self.frame_descriptors)?;
                self.last_evict = page_id_to_replace;
                frame = *self.page_table.get(&page_id_to_replace).unwrap();
                self.page_table.remove(&page_id_to_replace);
            };

            //ADD IN TABLE CORRESPONDING FRAME AND PAGE ID
            self.page_table.insert(pid, frame);
            frame_desc = self.frame_descriptors.index_mut(&frame);
            frame_desc.frame_id = frame;

            //IF FRAME is dirty -> write to disk
            if frame_desc.dirty {
                self.disk_manager.borrow_mut().write(frame_desc.page_id, self.pool.index(&frame))?;
                frame_desc.dirty = false;
            };

            //load the page in the buffer
            let page = self.pool.index_mut(&frame);
            self.disk_manager.borrow_mut().read(pid, page)?;
            frame_desc.page_id = pid;
        } else {
            //IF PAGE IS IN BUFFER
            frame = *self.page_table.get(&pid).unwrap();
            frame_desc = self.frame_descriptors.index_mut(&frame);
        };

        self.replacement_strat.on_pin(frame_desc);
        frame_desc.pin_count += 1;

        Ok(self.pool.index_mut(frame))
    }


    /// Unpins page *pid*.
    ///
    /// If no page with id *pid* is present in the page table, the function panics.
    /// The frame descriptor must be updated accordingly.
    ///
    /// # Safety
    /// For every [`BufferManager::unpin`] call for a page ID, there must be a corresponding
    /// [`BufferManager::pin`] call with the **same** page ID.
    ///
    /// # Panics
    /// May panic if page ID of `page` is not loaded in [`BufferManager`].
    fn unpin(&mut self, page_id: PageID, dirty: bool) {
        if !self.page_table.contains_key(&page_id) {
            panic!();
        };
        let frame = self.page_table.get(&page_id).unwrap();
        let frame_desc = self.frame_descriptors.index_mut(frame);
        frame_desc.dirty = dirty;
        if frame_desc.pin_count > 0 {
            frame_desc.pin_count -= 1;
        }
    }
}
