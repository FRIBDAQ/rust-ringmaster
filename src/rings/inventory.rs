///!
///! This module provides a mechanism to inventory
///! the rings in a specific directory.  The inventory
///! supports calling a closure for each file that is ring buffer
///! and a second closure for any file that is not a ringbuffer.
///

pub mod inventory {
    use nscldaq_ringbuffer::ringbuffer;
    use std::ffi::OsString;
    use std::fs;
    use std::fs::DirEntry;
    use std::io;
    use std::path::Path;
    ///
    /// Inventory the ringbuffers in a directory.
    /// This is done by reading the files in the directory
    /// and trying to map them as ringbuffer maps.
    /// Those that can be mapped call the is_ring closure
    /// Those that cannot be mapped call the not_ring closure.
    /// The name of the file as a string slice reference is
    /// passed to each of those closures.
    ///
    pub fn inventory_rings(dir_name: &str, is_ring: &Fn(&str), not_ring: &Fn(&str)) {
        let path = Path::new(dir_name);
        let iteration = fs::read_dir(path).unwrap();
        for file in iteration {
            let name = file.unwrap().path().into_os_string().into_string().unwrap();

            if let Ok(ring) = ringbuffer::RingBufferMap::new(&name) {
                is_ring(&name);
            } else {
                not_ring(&name);
            }
        }
    }
}
