pub mod rings {
    use std::sync;
    use std::thread;
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
        should_run: sync::atomic::AtomicBool,
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
                should_run: sync::atomic::AtomicBool::new(true),
                client_info: client,
            }
        }
        ///
        /// set_monitor should be called to receive the thread handle
        /// from the thread::spawn call.  Normally this will be
        /// look something like:
        ///
        /// ```
        ///  use nscldaq_ringmaster::rings::rings::rings::clap*
        ///  let some_client = Client::Producer{pid : 1234};  
        ///  let info = ClientMonitorInfo::new(some_client);
        ///  info.set_monitor(thread::spawn(|| {}));
        /// ```
        pub fn set_monitor(&mut self, handle: thread::JoinHandle<()>) {
            self.handle = Some(handle);
        }
    }
    /// Provides all of the information we, the ringmaster, need to know
    /// about a ringbuffer
    ///
    pub struct RingBufferInfo {
        ring_file: String,
        client_monitors: Vec<ClientMonitorInfo>,
    }
}
