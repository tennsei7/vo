use crate::core::scheduler::workpool::{TaskRunner, WorkerPool};
use crate::host::host::Host;

use crossbeam::queue::ArrayQueue;
//use crossbeam::utils::CachePadded;

pub struct NewScheduler {
    pool: WorkerPool,
    num_threads: usize,
    thread_hosts: Vec<ArrayQueue<Host>>,
    thread_hosts_processed: Vec<ArrayQueue<Host>>,
    hosts_need_swap: bool,
}

impl NewScheduler {
    pub fn new<T>(num_threads: u32, hosts: T) -> Self
    where
        T: IntoIterator<Item = Host>,
        <T as IntoIterator>::IntoIter: ExactSizeIterator,
    {
        let hosts = hosts.into_iter();

        let pool = WorkerPool::new(num_threads);

        // each thread gets two fixed-sized queues with enough capacity to store every host
        let thread_hosts: Vec<_> = (0..num_threads)
            .map(|_| ArrayQueue::new(hosts.len()))
            .collect();
        let thread_hosts_2: Vec<_> = (0..num_threads)
            .map(|_| ArrayQueue::new(hosts.len()))
            .collect();

        // assign hosts to threads in a round-robin manner
        for (thread_queue, host) in thread_hosts.iter().cycle().zip(hosts) {
            thread_queue.push(host).unwrap();
        }

        Self {
            pool,
            num_threads: num_threads as usize,
            thread_hosts,
            thread_hosts_processed: thread_hosts_2,
            hosts_need_swap: false,
        }
    }

    /// The maximum number of threads that will ever be run in parallel.
    pub fn parallelism(&self) -> usize {
        self.num_threads
    }

    /// A scope for any task run on the scheduler. The current thread will block at the end of the
    /// scope until the task has completed.
    pub fn scope<'scope>(
        &'scope mut self,
        f: impl for<'a, 'b> FnOnce(SchedScope<'a, 'b, 'scope>) + 'scope,
    ) {
        // we can't swap after the below `pool.scope()` due to lifetime restrictions, so we need to
        // do it before instead
        if self.hosts_need_swap {
            #[cfg(debug_assertions)]
            for queue in self.thread_hosts {
                assert_eq!(queue.len(), 0);
            }

            std::mem::swap(&mut self.thread_hosts, &mut self.thread_hosts_processed);
            self.hosts_need_swap = false;
        }

        // data/references that we'll pass to the scope
        let thread_hosts = &self.thread_hosts;
        let thread_hosts_processed = &self.thread_hosts_processed;
        let hosts_need_swap = &mut self.hosts_need_swap;

        // we cannot access `self` after calling `pool.scope()` since `SchedScope` has a lifetime of
        // `'scope` (which at minimum spans the entire function)

        self.pool.scope(move |s| {
            let sched_scope = SchedScope {
                thread_hosts,
                thread_hosts_processed,
                hosts_need_swap,
                runner: s,
            };

            (f)(sched_scope);
        });
    }

    /// Join all threads started by the scheduler.
    pub fn join(self) {
        self.pool.join();

        // when the host is in rust we won't need to do this
        for host_queue in self.thread_hosts.iter() {
            while let Some(host) = host_queue.pop() {
                use crate::cshadow as c;
                unsafe { c::host_unref(host.chost()) };
            }
        }
    }

    /*
    pub fn run(
        &mut self,
        f: impl Fn(&mut Host) + Send + Sync + Clone,
        background_f: impl FnOnce(),
    ) {
        self.pool.scope(|s| {
            s.run(|i| {
                let i = i as usize;
                for t in 0..self.num_threads {
                    while let Some(mut host) = self.thread_hosts[(i + t) % self.num_threads].pop() {
                        (f)(&mut host);
                        self.thread_hosts_processed[i].push(host);
                    }
                }
            });
            background_f();
        });

        std::mem::swap(&mut self.thread_hosts, &mut self.thread_hosts_processed);

        /*
        scope_all(self.threads.iter().map(|x| &x.thread), |scopes| {
            for (scope, hosts) in scopes.iter().zip(self.thread_hosts.iter_mut()) {
                let f = f.clone();
                if hosts.len() == 0 {
                    continue;
                }
                scope.spawn(move |_| {
                    //let a = std::time::Instant::now();
                    for host in hosts {
                        (f)(host);
                    }
                    //b += a.elapsed();
                });
            }
        });
        */
    }

    pub fn run_on_threads(&mut self, f: impl Fn(u32) + Sync + Send + Copy) {
        self.pool.scope(|s| {
            s.run(|i| (f)(i));
        });
    }
    */

    /*
    pub fn run_hosts_for_thread(&mut self, f: impl Fn(u32, Iterator<Item = &Host>) + Sync + Send + Copy) {
        self.pool.scope(|s| {
            s.run(|i| (f)(i, self.thread_hosts[i]));
        });
    }
    */

    /*
    pub fn run_map_reduce<T>(
        &mut self,
        map_fn: impl Fn(&mut Host) -> T + Send + Sync + Clone,
        reduce_fn: impl Fn(T, T) -> T + Send + Sync + Clone,
    ) -> Option<T> {
        let mut results: Vec<CachePadded<Option<T>>> = Vec::new();

        self.pool.scope(|s| {
            s.run(|i| {
                let i = i as usize;
                while let Some(mut host) = self.thread_hosts[i].pop() {
                    let result = (map_fn)(&mut host);
                    if let Some(val) = *results[i] {
                        *results[i] = Some((reduce_fn)(result, val));
                    } else {
                        *results[i] = Some(result);
                    }
                    self.thread_hosts_processed[i].push(host);
                }
            });
        });

        std::mem::swap(&mut self.thread_hosts, &mut self.thread_hosts_processed);

        let final_val = None;

        for result in results.into_iter() {
            if let Some(x) = final_val {
                if let Some(result) = *result {
                    final_val = Some((reduce_fn)(x, result));
                }
            } else {
                final_val = *result;
            }
        }

        final_val
    }
    */

    /*
    pub fn hosts(&self) -> impl Iterator<Item = &Host> {
        self.thread_hosts.iter().flatten()
    }
    */
}

pub struct SchedScope<'sched, 'pool, 'scope>
where
    'sched: 'scope,
{
    thread_hosts: &'sched Vec<ArrayQueue<Host>>,
    thread_hosts_processed: &'sched Vec<ArrayQueue<Host>>,
    hosts_need_swap: &'sched mut bool,
    runner: TaskRunner<'pool, 'scope>,
}

impl<'sched, 'pool, 'scope> SchedScope<'sched, 'pool, 'scope> {
    /*
    pub fn run(self, f: impl Fn(&mut Host) + Send + Sync + Clone + 'scope) {
        self.runner.run(move |i| {
            let i = i as usize;

            for t in 0..self.num_threads {
                while let Some(mut host) = self.thread_hosts[(i + t) % self.num_threads].pop() {
                    (f)(&mut host);
                    self.thread_hosts_processed[i].push(host);
                }
            }
        });
        *self.hosts_need_swap = true;
    }

    pub fn run_with_data<T>(
        self,
        elems: &'scope [T],
        f: impl Fn(&mut Host, &T) + Send + Sync + Clone + 'scope,
    ) where
        T: Sync,
    {
        self.runner.run(move |i| {
            let i = i as usize;
            let this_elem = &elems[i];

            for t in 0..self.num_threads {
                while let Some(mut host) = self.thread_hosts[(i + t) % self.num_threads].pop() {
                    (f)(&mut host, this_elem);
                    self.thread_hosts_processed[i].push(host);
                }
            }
        });
        *self.hosts_need_swap = true;
    }

    pub fn run_on_threads(self, f: impl Fn(u32) + Sync + Send + Copy + 'scope) {
        self.runner.run(move |i| (f)(i));
    }
    */

    /// Run the closure on all threads.
    pub fn run(self, f: impl Fn(usize) + Sync + Send + 'scope) {
        self.runner.run(move |i| (f)(i as usize));
    }

    /// Run the closure on all threads.
    ///
    /// You must iterate over the provided `HostIter` to completion (until `next()` returns `None`),
    /// otherwise this will panic.
    pub fn run_with_hosts(self, f: impl Fn(usize, &mut HostIter) + Send + Sync + 'scope) {
        self.runner.run(move |i| {
            let i = i as usize;

            let mut host_iter = HostIter {
                thread_hosts_from: &self.thread_hosts,
                thread_hosts_to: &self.thread_hosts_processed[i],
                this_thread_index: i,
                thread_index_iter_offset: 0,
                current_host: None,
            };

            f(i, &mut host_iter);

            assert!(host_iter.current_host.is_none());
            assert!(host_iter.next().is_none());
        });

        *self.hosts_need_swap = true;
    }

    /// Run the closure on all threads. The element given to the closure will not be given to any
    /// other thread while this closure is running, which means you should not expect any contention
    /// on this element if using interior mutability.
    ///
    /// You must iterate over the provided `HostIter` to completion (until `next()` returns `None`),
    /// otherwise this will panic.
    ///
    /// The provided slice must have a length of at least `NewScheduler::parallelism`. If the data
    /// needs to be initialized, it should be initialized before calling this function and not at
    /// the beginning of the closure.
    pub fn run_with_data<T>(
        self,
        elems: &'scope [T],
        f: impl Fn(usize, &mut HostIter, &T) + Send + Sync + 'scope,
    ) where
        T: Sync,
    {
        self.runner.run(move |i| {
            let i = i as usize;
            let this_elem = &elems[i];

            let mut host_iter = HostIter {
                thread_hosts_from: &self.thread_hosts[..],
                thread_hosts_to: &self.thread_hosts_processed[i],
                this_thread_index: i,
                thread_index_iter_offset: 0,
                current_host: None,
            };

            f(i, &mut host_iter, this_elem);

            assert!(host_iter.current_host.is_none());
            assert!(host_iter.next().is_none());
        });

        *self.hosts_need_swap = true;
    }
}

pub struct HostIter<'a> {
    /// Queues to take hosts from.
    thread_hosts_from: &'a [ArrayQueue<Host>],
    /// The queue to add hosts to when done with them.
    thread_hosts_to: &'a ArrayQueue<Host>,
    /// The index of this thread. This is the first queue of `thread_hosts_from` that we take hosts
    /// from.
    this_thread_index: usize,
    /// The thread offset of our iterator; stored so that we can resume where we left off.
    thread_index_iter_offset: usize,
    /// The host that was last returned from `next()`.
    current_host: Option<Host>,
}

impl<'a> HostIter<'a> {
    /// Get the next host.
    pub fn next(&mut self) -> Option<&mut Host> {
        // a generator would be nice here...
        let num_threads = self.thread_hosts_from.len();

        self.return_current_host();

        while self.thread_index_iter_offset < num_threads {
            let iter_thread_index = self.this_thread_index + self.thread_index_iter_offset;
            let queue = &self.thread_hosts_from[iter_thread_index % num_threads];

            match queue.pop() {
                Some(host) => {
                    // yield the host, but keep ownership so that we can add it back to the proper
                    // queue later
                    self.current_host = Some(host);
                    return self.current_host.as_mut();
                }
                // no hosts remaining, so move on to the next queue
                None => self.thread_index_iter_offset += 1,
            }
        }

        None
    }

    /// Returns the currently stored host back to a queue.
    fn return_current_host(&mut self) {
        if let Some(current_host) = self.current_host.take() {
            self.thread_hosts_to.push(current_host).unwrap();
        }
    }
}

impl<'a> std::ops::Drop for HostIter<'a> {
    fn drop(&mut self) {
        // make sure we don't own and drop a host
        self.return_current_host();
    }
}

/*
pub trait HostIter {
    fn next(&mut self) -> Option<&mut Host>;
}

pub struct SchedHostIter<'a> {
    thread_hosts: &'a Vec<ArrayQueue<Host>>,
    thread_hosts_processed: &'a Vec<ArrayQueue<Host>>,
    thread_index: usize,
    thread_index_offset: usize,
    current_host: Option<Host>,
}

impl<'a> HostIter for SchedHostIter<'a> {
    fn next(&mut self) -> Option<&mut Host> {
        let num_threads = self.thread_hosts.len();

        if let Some(current_host) = self.current_host.take() {
            self.thread_hosts_processed[self.thread_index].push(current_host);
        }

        while self.thread_index_offset < num_threads {
            match self.thread_hosts[(self.thread_index + self.thread_index_offset) % num_threads].pop() {
                Some(host) => {
                    self.current_host = Some(host);
                    return self.current_host.as_mut();
                },
                None => self.thread_index_offset += 1,
            }
        }

        None
    }
}

impl<'a> std::ops::Drop for SchedHostIter<'a> {
    fn drop(&mut self) {
        if let Some(current_host) = self.current_host.take() {
            self.thread_hosts_processed[self.thread_index].push(current_host);
        }
    }
}

pub struct NewScheduler {
    pool: WorkerPool,
    num_threads: usize,
    thread_hosts: Vec<ArrayQueue<Host>>,
    thread_hosts_processed: Vec<ArrayQueue<Host>>,
}

impl NewScheduler {
    pub fn new(num_threads: u32, hosts: impl IntoIterator<Item = Host>) -> Self {
        let pool = WorkerPool::new(num_threads);

        let hosts: Vec<_> = hosts.into_iter().collect();

        let mut thread_hosts: Vec<_> = (0..num_threads)
            .map(|_| ArrayQueue::new(hosts.len()))
            .collect();
        let mut thread_hosts_2: Vec<_> = (0..num_threads)
            .map(|_| ArrayQueue::new(hosts.len()))
            .collect();

        // assign hosts to threads in a round-robin manner
        for (i, host) in hosts.into_iter().enumerate() {
            let thread_index = i % (num_threads as usize);
            thread_hosts[thread_index].push(host);
        }

        Self {
            pool,
            num_threads: num_threads as usize,
            thread_hosts,
            thread_hosts_processed: thread_hosts_2,
        }
    }

    pub fn run(
        &mut self,
        //f: impl Fn(&mut Host) + Send + Sync + Clone,
        f: impl Fn(&mut dyn HostIter) + Send + Sync + Clone,
        background_f: impl FnOnce(),
    ) {
        self.pool.scope(|s| {
            s.run(|i| {
                let i = i as usize;
                /*
                for t in 0..self.num_threads {
                    while let Some(mut host) = self.thread_hosts[(i + t) % self.num_threads].pop() {
                        (f)(&mut host);
                        self.thread_hosts_processed[i].push(host);
                    }
                }
                */
                let mut host_iter = SchedHostIter {
                    thread_hosts: &self.thread_hosts,
                    thread_hosts_processed: &self.thread_hosts_processed,
                    thread_index: i,
                    thread_index_offset: 0,
                    current_host: None,
                };

                f(&mut host_iter);

                assert!(host_iter.current_host.is_none());
            });
            background_f();
        });

        std::mem::swap(&mut self.thread_hosts, &mut self.thread_hosts_processed);
    }

    pub fn run_on_threads(&mut self, f: impl Fn(u32) + Sync + Send + Copy) {
        self.pool.scope(|s| {
            s.run(|i| (f)(i));
        });
    }

    pub fn join(self) {
        self.pool.join();

        for host_queue in self.thread_hosts.iter() {
            while let Some(host) = host_queue.pop() {
                unsafe { crate::cshadow::host_unref(host.chost()) };
            }
        }
    }
}
*/

/*
pub struct NewScheduler {
    pool: rayon::ThreadPool,
    hosts: Vec<Host>,
}

impl NewScheduler {
    pub fn new(
        num_threads: u32,
        hosts: impl IntoIterator<Item = Host>,
        bootstrap_end_time: EmulatedTime,
    ) -> Self {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads as usize)
            .spawn_handler(|thread| {
                std::thread::spawn(move || {
                    //let mut cpu_set = nix::sched::CpuSet::new();
                    //cpu_set.set(thread.index()).unwrap();
                    //nix::sched::sched_setaffinity(nix::unistd::Pid::from_raw(0), &cpu_set).unwrap();

                    unsafe {
                        worker::Worker::new_for_this_thread(
                            worker::WorkerThreadID(thread.index() as u32),
                            bootstrap_end_time,
                        )
                    };
                    thread.run()
                });
                Ok(())
            })
            .build()
            .unwrap();

        Self {
            pool,
            hosts: hosts.into_iter().collect(),
        }
    }

    //pub fn run(&mut self, f: impl Fn(&mut Host) + Send + Sync + Clone, f2: impl Fn(&Host) -> bool + Send + Sync + Clone) {
    pub fn run(&mut self, f: impl Fn(&mut Host) + Send + Sync + Clone) {
        self.pool.in_place_scope(|scope| {
            for host in self.hosts.iter_mut() {
                //if !(f2)(host) {
                //    continue;
                //}
                let f = f.clone();
                scope.spawn(move |_| {
                    (f)(host);
                });
            }
        });
    }
}
*/

/*
pub struct NewScheduler {
    threads: Vec<ThreadInfo>,
    thread_hosts: Vec<Vec<Host>>,
}

impl NewScheduler {
    pub fn new(
        num_threads: u32,
        hosts: impl IntoIterator<Item = Host>,
        bootstrap_end_time: EmulatedTime,
    ) -> Self {
        let threads: Vec<_> = (0..num_threads)
            .map(|thread_id| ThreadInfo {
                thread: rayon::ThreadPoolBuilder::new()
                    .num_threads(1)
                    .spawn_handler(|thread| {
                        let mut cpu_set = nix::sched::CpuSet::new();
                        cpu_set.set(thread_id as usize).unwrap();
                        nix::sched::sched_setaffinity(nix::unistd::Pid::from_raw(0), &cpu_set)
                            .unwrap();

                        std::thread::spawn(move || {
                            unsafe {
                                worker::Worker::new_for_this_thread(
                                    worker::WorkerThreadID(thread.index() as u32),
                                    bootstrap_end_time,
                                )
                            };
                            thread.run()
                        });
                        Ok(())
                    })
                    .build()
                    .unwrap(),
                //hosts: Vec::new(),
            })
            .collect();

        let mut thread_hosts: Vec<_> = threads.iter().map(|_| Vec::new()).collect();

        // assign hosts to threads in a round-robin manner
        for (i, host) in hosts.into_iter().enumerate() {
            let thread_index = i % threads.len();
            thread_hosts[thread_index].push(host);
        }

        Self {
            threads,
            thread_hosts,
        }
    }

    //pub fn run(&mut self, f: impl Fn(&mut Host) + Send + Sync + Clone, _f2: impl Fn(&Host) -> bool + Send + Sync + Clone) {
    pub fn run(&mut self, f: impl Fn(&mut Host) + Send + Sync + Clone) {
        //let start = std::time::Instant::now();
        //let mut first = std::time::Duration::ZERO;
        // TODO: this scope_all() is slow
        //scope_all(self.threads.iter().map(|x| &x.thread).take(15), |scopes| {
        scope_all(self.threads.iter().map(|x| &x.thread), |scopes| {
            //first = start.elapsed();
            for (scope, hosts) in scopes.iter().zip(self.thread_hosts.iter_mut()) {
                let f = f.clone();
                if hosts.len() == 0 {
                    continue;
                }
                scope.spawn(move |_| {
                    //let a = std::time::Instant::now();
                    for host in hosts {
                        (f)(host);
                    }
                    //b += a.elapsed();
                });
            }
        });
        //let end = start.elapsed();
        //log::warn!("STEVE: {:?}/{:?}", first, start.elapsed());
        //log::warn!("STEVE: {b:?}/{end:?} ({:?})", b/end);
    }

    pub fn hosts(&self) -> impl Iterator<Item = &Host> {
        self.thread_hosts.iter().flatten()
    }
}

struct ThreadInfo {
    thread: rayon::ThreadPool,
}

#[inline]
fn scope_all<'a, 'scope>(
    pools: impl ExactSizeIterator<Item = &'a rayon::ThreadPool> + Send,
    f: impl FnOnce(Vec<&rayon::Scope<'scope>>) + Send + 'scope,
) {
    #[inline]
    fn recursive_scope<'a, 'scope>(
        mut pools: impl Iterator<Item = &'a rayon::ThreadPool> + Send,
        scopes: Vec<&rayon::Scope<'scope>>,
        f: impl FnOnce(Vec<&rayon::Scope<'scope>>) + Send + 'scope,
    ) {
        match pools.next() {
            None => return f(scopes),
            Some(pool) => {
                pool.in_place_scope(move |s| {
                    let mut scopes = scopes;
                    scopes.push(s);
                    recursive_scope(pools, scopes, f);
                });
            }
        }
    }

    let vec = Vec::with_capacity(pools.len());
    recursive_scope(pools, vec, f)
}
*/
