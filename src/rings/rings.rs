pub mod rings {
    use std::collections::HashMap;
    use std::sync;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;
    use sysinfo::{Pid, PidExt, ProcessExt, System};
    ///
    /// This enum provides information about the
    /// way a client is attached to a ring:
    ///  
    /// *  pid is the process id of the client.
    /// *  slot is the consumer slot for a consumer client.
    ///
    #[derive(Copy, Clone, Debug)]
    pub enum Client {
        Producer { pid: u32 },
        Consumer { pid: u32, slot: u32 },
    }
    ///
    /// provides the information we need to know about a
    /// ringmaster client monitor thread.
    ///
    /// *   handle -is the join handle for a monitor thread.
    /// *   should_run - is the flag that will be initialized to ```true```
    /// and set to false to request the thread exit.
    ///

    pub struct ClientMonitorInfo {
        handle: Option<thread::JoinHandle<()>>,
        should_run: bool,
        client_info: Client,
    }
    impl ClientMonitorInfo {
        ///
        /// prepares a ClientMonitorInfo struct. Note that
        /// we don't have a monitor thread yet.  This is
        /// added by set_monitor.  This is necessary because we don't
        /// want a race condition between setting up the should_run
        /// atomic bool and the thread  referencing for the first time.
        /// The thread needa that initialized but it does not need
        /// its own thread handle.
        ///
        pub fn new(client: Client) -> ClientMonitorInfo {
            ClientMonitorInfo {
                handle: None,
                should_run: true,
                client_info: client,
            }
        }
        ///
        /// set_monitor should be called to receive the thread handle
        /// from the thread::spawn call.  Normally this will be
        /// look something like:
        ///
        /// ```
        ///  use nscldaq_ringmaster::rings::rings::rings::*;
        ///  use std::thread;
        ///
        ///  let some_client = Client::Producer{pid : 1234};  
        ///  let mut info = ClientMonitorInfo::new(some_client);
        ///  info.set_monitor(thread::spawn(|| {}));
        /// ```
        pub fn set_monitor(&mut self, handle: thread::JoinHandle<()>) {
            self.handle = Some(handle);
        }

        ///
        /// stop_monitor
        ///    Requests that the monitor thread stop and blocks
        /// (via join) until the monitor thread actually does stop
        ///
        /// Note that if the handle is not set yet (the thread not spawned),
        /// we're just going to return right away since it's assumed the
        /// thread never started.
        /// This juggling is done because we need to avoid deadlock
        /// in the thread.
        /// The me parameter is a arc/mutext encapsulating the
        /// client info to operate on.
        ///
        pub fn stop_monitor(me: &mut Arc<Mutex<Self>>) {
            me.lock().unwrap().should_run = false;
            //
            // Note that the code below leaves self.handle = None
            // which is cool since we can then support multiple stop_monitor
            // calls just fine.

            if me.lock().unwrap().handle.is_none() {
                me.lock().unwrap().should_run = true;
                return;
            }
            // Key point this loop allows the mutex to be
            // unlocked from time to time.
            loop {
                let lock = me.lock().unwrap();
                if let Some(handle) = &lock.handle {
                    if handle.is_finished() {
                        break;
                    } else {
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            // Now we can join:

            me.lock().unwrap().handle.take().unwrap().join().unwrap();
        }
        ///
        /// Determine if a monitor should keep running:
        ///
        fn keep_running(&self) -> bool {
            return self.should_run;
        }
    }
    /// Provides all of the information we, the ringmaster, need to know
    /// about a ringbuffer
    ///
    pub struct RingBufferInfo {
        ring_file: String,
        client_monitors: HashMap<u32, Arc<Mutex<ClientMonitorInfo>>>,
    }
    impl RingBufferInfo {
        #[cfg(target_os = "linux")]
        fn kill_pid(pid: u32) {
            let sys_pid = Pid::from_u32(pid);
            let mut s = sysinfo::System::new_all();
            if let Some(process) = s.process(sys_pid) {
                process.kill(); // Do the best we can.
            }
        }
        #[cfg(not(target_os = "linux"))]
        fn kill_pid(pid: u32) {} // Else can't on windows
        ///
        ///  creates the object.  We initially have the ring file
        /// path and then an empty client monitors collection.
        /// As we add clients to the ring we make entries into the
        /// client_monitors collection. If a monitor
        /// must be removed we take it out of the list.
        ///
        pub fn new(ring: &str) -> RingBufferInfo {
            RingBufferInfo {
                ring_file: String::from(ring),
                client_monitors: HashMap::new(),
            }
        }
        ///
        /// Add a new client to the ring buffer.
        /// The thread must have been started (if there will be one)
        /// by our client.
        pub fn add_client(
            &mut self,
            client: &Arc<Mutex<ClientMonitorInfo>>,
        ) -> &mut RingBufferInfo {
            let key = match client.lock().unwrap().client_info {
                Client::Producer { pid } => pid,
                Client::Consumer { pid, slot } => pid,
            };
            self.client_monitors.insert(key, Arc::clone(client));

            self
        }
        ///
        /// Remove a client from the ring buffer given its
        /// PID.  
        /// *  The monitor's thread is halted.
        /// *  If possible, the process is killed.
        ///
        pub fn remove_client(&mut self, pid: u32) -> &mut RingBufferInfo {
            let info = self.client_monitors.remove(&pid);
            if let Some(mut client) = info {
                ClientMonitorInfo::stop_monitor(&mut client);
                Self::kill_pid(pid);
            }
            self
        }
        /// Convenience method to kill all clients.
        ///
        pub fn remove_all(&mut self) -> &mut RingBufferInfo {
            let mut pids: Vec<u32> = Vec::new();
            // Collect the pids:
            for pid in self.client_monitors.keys() {
                pids.push(*pid);
            }

            for pid in pids {
                self.remove_client(pid);
            }

            self
        }
    }
    #[cfg(test)]
    // Tests for ClienMonitorInfo:

    mod clmoninfo_tests {
        use super::*;
        use std::sync::{Arc, Mutex};
        use std::thread;
        use std::thread::sleep;
        use std::time::Duration;

        #[test]
        fn new_1() {
            let c = Client::Producer { pid: 124 };
            let info = ClientMonitorInfo::new(c);
            assert!(info.handle.is_none());
            if let Client::Producer { pid } = info.client_info {
                assert_eq!(124, pid);
            } else {
                assert!(false, "Wrong type of client encapsulated");
            }
            assert!(info.should_run);
        }
        #[test]
        fn new_2() {
            let c = Client::Consumer { pid: 123, slot: 3 };
            let info = ClientMonitorInfo::new(c);
            assert!(info.handle.is_none());
            if let Client::Consumer { pid, slot } = info.client_info {
                assert_eq!(123, pid);
                assert_eq!(3, slot);
            } else {
                assert!(false, "Wrong type of client encapsulated");
            }
            assert!(info.should_run);
        }
        #[test]
        fn set_monitor_1() {
            let client = Client::Producer { pid: 1234 };
            let mut info = ClientMonitorInfo::new(client);

            info.set_monitor(thread::spawn(|| {}));
            assert!(info.handle.is_some());
            if let Some(h) = info.handle {
                assert!(h.join().is_ok());
            }
        }
        #[test]
        fn stop_monitor_1() {
            let client = Client::Producer { pid: 1234 };
            let info = ClientMonitorInfo::new(client);
            let mut my_safe = Arc::new(Mutex::new(info));
            let safe_info = Arc::clone(&my_safe);
            my_safe.lock().unwrap().set_monitor(thread::spawn(move || {
                for i in 1..100 {
                    println!("{}", i);
                    if safe_info.lock().unwrap().keep_running() {
                        println!("Sleeping again");
                        sleep(Duration::from_millis(100));
                    } else {
                        println!("Exiting");
                        return;
                    }
                }
            }));
            assert!(my_safe.lock().unwrap().should_run);
            ClientMonitorInfo::stop_monitor(&mut my_safe);
            assert!(!my_safe.lock().unwrap().should_run);
        }
    }
}
