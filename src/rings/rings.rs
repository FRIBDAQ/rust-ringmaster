pub mod rings {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[cfg(target_os = "linux")]
    use sysinfo::{Pid, ProcessExt, Signal, System, SystemExt};
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
        pub should_run: bool,
        pub client_info: Client,
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
            self.should_run = true;
            self.handle = Some(handle);
        }
        /// Schedule the monitor to stop
        ///  but don't wait for it
        ///
        pub fn schedule_stop_monitor(me: &mut Arc<Mutex<Self>>) {
            me.lock().unwrap().should_run = false;
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

            // Can'figure out how to join without deadlock.
        }
        ///
        /// Determine if a monitor should keep running:
        ///
        pub fn keep_running(&self) -> bool {
            return self.should_run;
        }
    }
    /// Provides all of the information we, the ringmaster, need to know
    /// about a ringbuffer
    ///
    pub struct RingBufferInfo {
        pub ring_file: String,
        client_monitors: HashMap<u32, Arc<Mutex<ClientMonitorInfo>>>,
    }
    impl RingBufferInfo {
        #[cfg(target_os = "linux")]
        fn kill_pid(pid: u32) {
            let sys_pid = pid as Pid; // Pid::from_u32(pid);
            let mut s = sysinfo::System::new_all();
            for (ppid, proc) in s.get_processes() {
                if *ppid == sys_pid {
                    proc.kill(sysinfo::Signal::Kill);
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        fn kill_pid(_pid: u32) {} // Else can't on windows but need fn for compiler
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
        /// Check existence of a pid
        ///
        pub fn have_pid(&self, pid: u32) -> bool {
            self.client_monitors.contains_key(&pid)
        }
        ///
        /// Get the client information associated with a pid in the ringL
        ///
        pub fn get_client_info(&mut self, pid: &u32) -> Option<&Arc<Mutex<ClientMonitorInfo>>> {
            self.client_monitors.get(&pid)
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
                Client::Consumer { pid, slot: _slot } => pid,
            };
            self.client_monitors.insert(key, Arc::clone(client));

            self
        }
        /// unlist client
        ///
        pub fn unlist_client(&mut self, pid: u32) -> &mut RingBufferInfo {
            if let Some(_) = self.client_monitors.remove(&pid) {}
            self
        }
        /// Remove a client from a ring buffer given its pid.
        ///  
        /// *  Halt the monitor thread.
        /// *  *Don't* kill the process.
        ///
        /// If the pid does not have an entry this is a silent no-op.
        ///
        pub fn unregister_client(&mut self, pid: u32) -> &mut RingBufferInfo {
            if let Some(mut info) = self.client_monitors.remove(&pid) {
                ClientMonitorInfo::schedule_stop_monitor(&mut info)
            }
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
    #[cfg(test)]
    mod ringbuf_info_tests {
        use super::*;
        use std::thread::sleep;
        use std::time::Duration;
        #[test]
        fn new_1() {
            let info = RingBufferInfo::new("ringname");
            assert_eq!(String::from("ringname"), info.ring_file);
            assert_eq!(0, info.client_monitors.len());
        }
        #[test]
        fn add_1() {
            // add client information with no overwrite.

            let mut info = RingBufferInfo::new("ringbuffer");
            let producer = ClientMonitorInfo::new(Client::Producer { pid: 1234 });
            let arc = Arc::<Mutex<ClientMonitorInfo>>::new(Mutex::new(producer));
            info.add_client(&arc);
            assert_eq!(1, info.client_monitors.len());
            if let Some(arc) = info.client_monitors.get(&1234) {
                match arc.lock().unwrap().client_info {
                    Client::Producer { pid } => {
                        assert_eq!(1234, pid);
                    }
                    Client::Consumer {
                        pid: _pid,
                        slot: _slot,
                    } => {
                        assert!(false, "Got consumer expected producer");
                    }
                }
                assert!(arc.lock().unwrap().handle.is_none());
                assert!(arc.lock().unwrap().should_run);
            } else {
                assert!(false, "Did not insert client into map");
            }
        }
        #[test]
        fn add_2() {
            // Add a consumer client to the ring:
            let mut info = RingBufferInfo::new("ringbuffer");
            let consumer = ClientMonitorInfo::new(Client::Consumer { pid: 1234, slot: 2 });
            let arc = Arc::<Mutex<ClientMonitorInfo>>::new(Mutex::new(consumer));
            info.add_client(&arc);
            assert_eq!(1, info.client_monitors.len());
            if let Some(arc) = info.client_monitors.get(&1234) {
                match arc.lock().unwrap().client_info {
                    Client::Producer { pid: _pid } => {
                        assert!(false, "Should have gotten consumer, got producer");
                    }
                    Client::Consumer { pid, slot } => {
                        assert_eq!(1234, pid);
                        assert_eq!(2, slot);
                    }
                }
            } else {
                assert!(false, "Did not insert client into map!");
            }
        }
        #[test]
        fn add_3() {
            // add a consumer and producer - non colliding.

            let mut info = RingBufferInfo::new("ringbuffer");
            let producer = ClientMonitorInfo::new(Client::Producer { pid: 1111 });
            let consumer = ClientMonitorInfo::new(Client::Consumer { pid: 1234, slot: 2 });

            let arc_producer = Arc::new(Mutex::new(producer));
            let arc_consumer = Arc::new(Mutex::new(consumer));

            info.add_client(&arc_producer).add_client(&arc_consumer);

            assert_eq!(2, info.client_monitors.len());

            // we'll take it for granted that if inserted they're both
            // ok based on add_1, and add_2

            if let Some(_p) = info.client_monitors.get(&1111) {
                assert!(true);
            } else {
                assert!(false, "Producer did not get inserted");
            }

            if let Some(_c) = info.client_monitors.get(&1234) {
                assert!(true);
            } else {
                assert!(false, "Consumer did not get inserted");
            }
        }
        #[test]
        fn add_4() {
            // Second add overwrites existing add..
            let mut info = RingBufferInfo::new("ringbuffer");
            let producer = ClientMonitorInfo::new(Client::Producer { pid: 1234 });
            let consumer = ClientMonitorInfo::new(Client::Consumer { pid: 1234, slot: 2 });

            let arc_producer = Arc::new(Mutex::new(producer));
            let arc_consumer = Arc::new(Mutex::new(consumer));

            info.add_client(&arc_producer).add_client(&arc_consumer); // should overwrite.

            assert_eq!(1, info.client_monitors.len());
            if let Some(c) = info.client_monitors.get(&1234) {
                match c.lock().unwrap().client_info {
                    Client::Producer { pid: _pid } => {
                        assert!(false, "should have been a consumer");
                    }
                    Client::Consumer { pid, slot } => {
                        assert_eq!(1234, pid);
                        assert_eq!(2, slot);
                    }
                }
            } else {
                assert!(false, "There should be ! 1234 client but isn't");
            }
        }
        #[test]
        fn remove_1() {
            // Remove is ok if there's no client with that pid
            // to remove (silently does nothing)
            let mut info = RingBufferInfo::new("ring");
            info.remove_client(1234); // Should not panic.
        }
        #[test]
        fn remove_2() {
            // Remove when monitor process was not started works:
            let mut info = RingBufferInfo::new("ringbuffer");
            let producer = ClientMonitorInfo::new(Client::Producer { pid: 1234 });
            let arc_producer = Arc::new(Mutex::new(producer));
            info.add_client(&arc_producer).remove_client(1234);

            // Should be an empty client list:

            assert_eq!(0, info.client_monitors.len());
        }
        #[test]
        fn remove_3() {
            // Remove stops the monitor:

            let mut info = RingBufferInfo::new("ringbuffer");
            let producer = ClientMonitorInfo::new(Client::Producer { pid: 1234 });
            let arc_producer = Arc::new(Mutex::new(producer));
            let child_producer = Arc::clone(&arc_producer);
            arc_producer
                .lock()
                .unwrap()
                .set_monitor(thread::spawn(move || loop {
                    if child_producer.lock().unwrap().should_run {
                        sleep(Duration::from_millis(100));
                    } else {
                        return;
                    }
                }));
            // Now if we remove the client it should stop the thread.

            info.add_client(&arc_producer).remove_client(1234);
            assert_eq!(0, info.client_monitors.len());
        }
        #[test]
        fn remove_4() {
            // Remove all clients.. in this case two without any
            // actual monitor threads.

            let mut info = RingBufferInfo::new("ringbuffer");
            let producer = ClientMonitorInfo::new(Client::Producer { pid: 4321 });
            let consumer = ClientMonitorInfo::new(Client::Consumer { pid: 1234, slot: 2 });

            let arc_producer = Arc::new(Mutex::new(producer));
            let arc_consumer = Arc::new(Mutex::new(consumer));

            info.add_client(&arc_producer)
                .add_client(&arc_consumer)
                .remove_all();

            assert_eq!(0, info.client_monitors.len());
        }
    }
}
