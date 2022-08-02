//! Process management syscalls

use crate::config::MAX_SYSCALL_NUM;
use crate::loader::get_app_data_by_name;
use crate::mm::{translated_refmut, translated_str, convert_to_physical_addr};
use crate::task::{
    current_user_token, exit_current_and_run_next, mmap,
    munmap, suspend_current_and_run_next, TaskStatus, current_task, add_task, TaskControlBlock,
};
use crate::timer::get_time_us;
use alloc::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub status: TaskStatus,
    pub syscall_times: [u32; MAX_SYSCALL_NUM],
    pub time: usize,
}

impl From<TimeVal> for usize {
    fn from(tv: TimeVal) -> Self {
        tv.sec * 1_000_000 + tv.usec
    }
}

pub fn sys_exit(exit_code: i32) -> ! {
    debug!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    current_task().unwrap().pid.0 as isize
}

/// Syscall Fork which returns 0 for child process and child_pid for parent process
pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

/// Syscall Exec which accepts the elf path
pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    // find a child process

    // ---- access current TCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB lock exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after removing from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child TCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB lock automatically
}

// YOUR JOB: 引入虚地址后重写 sys_get_time
pub fn sys_get_time(_ts: *mut TimeVal, _tz: usize) -> isize {
    let us = get_time_us();
    // 将虚拟地址转换为物理地址
    let token = current_user_token();
    let ts = convert_to_physical_addr(token, _ts as usize) as *mut TimeVal;
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

// CLUE: 从 ch4 开始不再对调度算法进行测试~
pub fn sys_set_priority(_prio: isize) -> isize {
    -1
}

// YOUR JOB: 引入虚地址后重写 sys_task_info
pub fn sys_task_info(_ti: *mut TaskInfo) -> isize {
    // let first_sched_ts = get_first_sched_time();
    // let syscall_times = list_syscall_counts();
    // let curr_ts = get_time_us();
    // let token = current_user_token();
    // let ti = convert_to_physical_addr(token, _ti as usize) as *mut TaskInfo;
    // unsafe {
    //     *ti = TaskInfo {
    //         status: TaskStatus::Running,
    //         syscall_times: syscall_times,
    //         time: (usize::from(curr_ts) - usize::from(first_sched_ts)) / 1000,
    //     }
    // }
    0
}

// YOUR JOB: 扩展内核以实现 sys_mmap 和 sys_munmap
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    mmap(_start, _len, _port)
}

pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    munmap(_start, _len)
}

//
// YOUR JOB: 实现 sys_spawn 系统调用
// ALERT: 注意在实现 SPAWN 时不需要复制父进程地址空间，SPAWN != FORK + EXEC
pub fn sys_spawn(_path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, _path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = Arc::new(TaskControlBlock::new(data));
        let mut inner = task.inner_exclusive_access();
        let parent = current_task().unwrap();
        let mut parent_inner = parent.inner_exclusive_access();
        inner.parent = Some(Arc::downgrade(&parent));
        parent_inner.children.push(task.clone());
        drop(parent_inner);
        drop(inner);
        let pid = task.pid.0 as isize;
        task.exec(data);
        add_task(task);
        pid
    } else {
        -1
    }
}
