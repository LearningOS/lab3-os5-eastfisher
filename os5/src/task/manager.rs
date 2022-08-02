//! Implementation of [`TaskManager`]
//!
//! It is only used to manage processes and schedule process based on ready queue.
//! Other CPU process monitoring functions are in Processor.

use core::convert::TryFrom;

use super::{current_task, TaskControlBlock};
use crate::mm::{MapPermission, VirtAddr, VPNRange};
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;

pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl TryFrom<usize> for MapPermission {
    type Error = ();

    fn try_from(port: usize) -> Result<Self, Self::Error> {
        // 第 0 位表示是否可读，第 1 位表示是否可写，第 2 位表示是否可执行。其他位无效且必须为 0
        // port & !0x7 != 0 (port 其余位必须为0)
        // port & 0x7 = 0 (这样的内存无意义)
        if (port & !0x7) != 0 || (port & 0x7) == 0 {
            return Err(());
        }
        let mut permission = MapPermission::U;
        if port & 1 != 0 {
            permission |= MapPermission::R;
        }
        if port & 2 != 0 {
            permission |= MapPermission::W;
        }
        if port & 4 != 0 {
            permission |= MapPermission::X;
        }
        Ok(permission)
    }
}

// YOUR JOB: FIFO->Stride
/// A simple FIFO scheduler.
impl TaskManager {
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }

    // LAB2
    pub fn mmap(&self, start: usize, len: usize, port: usize) -> isize {
        // TODO
        // start 需要映射的虚存起始地址，要求按页对齐
        // len 映射字节长度，可以为 0
        // port：第 0 位表示是否可读，第 1 位表示是否可写，第 2 位表示是否可执行。其他位无效且必须为 0
        let start_va = VirtAddr::from(start);
        // start 没有按页大小对齐
        if start_va.page_offset() != 0 {
            return -1;
        }
        let end_va = VirtAddr::from(start + len);
        let permission = MapPermission::try_from(port);
        if let Err(_) = permission {
            return -1;
        }
        let perm = permission.unwrap();

        // len为0, 直接返回成功
        if len == 0 {
            return 0;
        }

        let current_task = current_task().unwrap();
        let memory_set = &mut current_task.inner_exclusive_access().memory_set;
        let vpn_start = start_va.floor();
        let vpn_end = end_va.ceil();
        let vpn_range = VPNRange::new(vpn_start, vpn_end);

        // [start, start + len) 中存在已经被映射的页
        for vpn in vpn_range {
            if let Some(pte) = memory_set.translate(vpn) {
                if pte.is_valid() {
                    return -1;
                }
            }
        }

        memory_set.insert_framed_area(start_va, end_va, perm);
        0
    }

    pub fn munmap(&self, start: usize, len: usize) -> isize {
        let start_va = VirtAddr::from(start);
        // start 没有按页大小对齐
        if start_va.page_offset() != 0 {
            return -1;
        }
        let end_va = VirtAddr::from(start + len);
        // len为0, 直接返回成功
        if len == 0 {
            return 0;
        }

        let current_task = current_task().unwrap();
        let memory_set = &mut current_task.inner_exclusive_access().memory_set;
        let vpn_start = start_va.floor();
        let vpn_end = end_va.ceil();
        let vpn_range = VPNRange::new(vpn_start, vpn_end);

        // [start, start + len) 中存在未被映射的虚存。
        for vpn in vpn_range {
            if let Some(pte) = memory_set.translate(vpn) {
                if !pte.is_valid() {
                    return -1;
                }
            } else {
                return -1;
            }
        }

        for vpn in vpn_range {
            memory_set.munmap(vpn);
        }
        0
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

pub fn add_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.exclusive_access().add(task);
}

pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}

// LAB2
pub fn mmap(start: usize, len: usize, port: usize) -> isize {
    TASK_MANAGER.exclusive_access().mmap(start, len, port)
}

pub fn munmap(start: usize, len: usize) -> isize {
    TASK_MANAGER.exclusive_access().munmap(start, len)
}
