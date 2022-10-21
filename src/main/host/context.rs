//! This module provides several *Context* structs, intended to bundle together
//! current relevant objects in the hierarchy. These are meant to replace
//! `worker_getActiveThread`, etc. Passing around the current context explicitly
//! instead of putting them in globals both allows us to avoid interior mutability
//! (and its associated runtime cost and fallibility), and lets us keep a
//! hierarchical object structure (e.g. allow holding a mutable Process and a
//! mutable Thread belonging to that process simultaneously).
//!
//! Most code (e.g. syscall handlers) can take a `ThreadContext` argument and use
//! that to manipulate anything on the Host. The *current* `Thread` and `Process`
//! should typically be accessed directly. e.g. since a mutable reference to the
//! current `Thread` exists at `ThreadContext::thread`, it *cannot* also be
//! accessible via `ThreadContext::process` or `ThreadContext::host`.
//!
//! The manner in which they're unavailable isn't implemented yet, but the current
//! plan is that they'll be temporarily removed from their collections. e.g. something
//! conceptually like:
//!
//! ```ignore
//! impl Process {
//!     pub fn continue_thread(&mut self, host_ctx: &mut HostContext, tid: ThreadId) {
//!         let thread = self.threads.get_mut(tid).take();
//!         thread.continue(&mut host_ctx.add_process(self));
//!         self.threads.get_mut(tid).replace(thread);
//!     }
//! }
//! ```
//!
//! The Context objects are designed to allow simultaneously borrowing from multiple
//! of their objects.  This is currently implemented by exposing their fields
//! directly - Rust then allows each field to be borrowed independently. This could
//! alternatively be implemented by providing methods that borrow some or all of
//! their internal references simultaneously.

use super::{
    host::HostRef,
    process::ProcessRef,
    thread::{CThread, Thread},
};
use crate::cshadow;

/// Represent the "current" Host.
pub struct HostContext<'a> {
    // We expose fields directly rather than through accessors, so that
    // users can borrow from each field independently.
    pub host: &'a mut HostRef,
}

impl<'a> HostContext<'a> {
    pub fn new(host: &'a mut HostRef) -> Self {
        Self { host }
    }

    /// Add the given process to the context.
    pub fn with_process(&'a mut self, process: &'a mut ProcessRef) -> ProcessContext<'a> {
        ProcessContext::new(self.host, process)
    }
}

/// Represent the "current" `Host` and `Process`.
pub struct ProcessContext<'a> {
    pub host: &'a mut HostRef,
    pub process: &'a mut ProcessRef,
}

impl<'a> ProcessContext<'a> {
    pub fn new(host: &'a mut HostRef, process: &'a mut ProcessRef) -> Self {
        Self { host, process }
    }

    pub fn with_thread(&'a mut self, thread: &'a mut dyn Thread) -> ThreadContext<'a> {
        ThreadContext::new(self.host, self.process, thread)
    }
}

/// Represent the "current" `Host`, `Process`, and `Thread`.
pub struct ThreadContext<'a> {
    pub host: &'a mut HostRef,
    pub process: &'a mut ProcessRef,
    pub thread: &'a mut dyn Thread,
}

impl<'a> ThreadContext<'a> {
    pub fn new(
        host: &'a mut HostRef,
        process: &'a mut ProcessRef,
        thread: &'a mut dyn Thread,
    ) -> Self {
        Self {
            host,
            process,
            thread,
        }
    }
}

/// Shadow's C code doesn't know about contexts. In places where C code calls
/// Rust code, we can build them from C pointers.
pub struct ThreadContextObjs {
    host: HostRef,
    process: ProcessRef,
    thread: CThread,
}

impl ThreadContextObjs {
    pub unsafe fn from_syscallhandler(sys: *mut cshadow::SysCallHandler) -> Self {
        let sys = unsafe { sys.as_mut().unwrap() };
        let host = unsafe { HostRef::borrow_from_c(sys.host) };
        let process = unsafe { ProcessRef::borrow_from_c(sys.process) };
        let thread = unsafe { CThread::new(sys.thread) };
        Self {
            host,
            process,
            thread,
        }
    }

    pub unsafe fn from_thread(thread: *mut cshadow::Thread) -> Self {
        let sys = unsafe { cshadow::thread_getSysCallHandler(thread) };
        let sys = unsafe { sys.as_mut().unwrap() };
        let host = unsafe { HostRef::borrow_from_c(sys.host) };
        let process = unsafe { ProcessRef::borrow_from_c(sys.process) };
        let thread = unsafe { CThread::new(sys.thread) };
        Self {
            host,
            process,
            thread,
        }
    }

    pub fn borrow(&mut self) -> ThreadContext {
        ThreadContext::new(&mut self.host, &mut self.process, &mut self.thread)
    }
}
