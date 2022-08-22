//! The *rings* module provides a database of known ringbuffer
//! files and the stuff we need to know.  These include:
//!
//! *  The paths to the files themselves.  While the ringmaster process
//! is being written so that those files are all in the same directory
//! really clever people will have noticed that in non POSIX shared
//! memory systems, where this all gets done with mmap(2), there's
//! nothing to stop you from making a ringbuffer in a subdirectory.
//! Or, even a ringbuffer in some other directory tree if REGISTER
//! understands that .
//! *  Information about the clients that are know to be attached to
//! those rings.  For the most part, that is the set of thread handles
//! that represent threads that are monitoring client exits and
//! the variable used to ask a thread to exit.  
//!
pub mod rings;
pub use self::rings::rings::*;
