///!
///! This module provides a mechanism to inventory
///! the rings in a specific directory.  The inventory
///! supports calling a closure for each file that is ring buffer
///! and a second closure for any file that is not a ringbuffer.
///

pub mod inventory {
    use nscldaq_ringbuffer::ringbuffer;
    use std::fs;
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
    pub fn inventory_rings(
        dir_name: &str,
        is_ring: &mut dyn FnMut(&str),
        not_ring: &mut dyn FnMut(&str),
    ) {
        let path = Path::new(dir_name);
        let iteration = fs::read_dir(path).unwrap();
        for file in iteration {
            let name = file.unwrap().path().into_os_string().into_string().unwrap();

            if let Ok(_ring) = ringbuffer::RingBufferMap::new(&name) {
                is_ring(&name);
            } else {
                not_ring(&name);
            }
        }
    }
    #[cfg(test)]
    mod inv_test {
        use super::*;
        use std::path::Path;
        // Note that I _think_ the working dir is the project top dir
        // if these are run via cargo.

        // closures that are useful to tests:

        fn collect_names(name: &str, collection: &mut Vec<String>) {
            collection.push(String::from(name));
        }
        #[test]
        fn inv_1() {
            let mut not_rings = Vec::<String>::new();
            let mut rings = Vec::<String>::new();
            inventory_rings(
                ".",
                &mut |name| collect_names(name, &mut rings),
                &mut |name| collect_names(name, &mut not_rings),
            );
            assert_eq!(1, rings.len());
            let p = Path::new(&rings[0]);

            assert!(p.ends_with("poop"));
        }
    }
}
