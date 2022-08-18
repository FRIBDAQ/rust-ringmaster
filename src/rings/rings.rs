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
    enum Client {
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
    struct ClientMonitorInfo {
        handle: thread::JoinHandle<()>,
        should_run: sync::atomic::AtomicBool,
        client_info: Client,
    }
    /// Provides all of the information we, the ringmaster, need to know
    /// about a ringbuffer
    ///
    pub struct RingBufferInfo {
        ring_file: String,
        client_monitors: Vec<ClientMonitorInfo>,
    }
    
}
