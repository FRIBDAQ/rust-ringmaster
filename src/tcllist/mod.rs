//!
//! The tcllist module provides the ability to build and
//! format simple Tcl lists.  The lists are simple in the sense
//! that
//! *   The main list is surrounded by {}.
//! *   It is assumed each element of the list requires no quoting execpt:
//! *   Sublists are surrounded by {} as well.
//!
//! Normally you'd create a list and then add to it.   You can add either
//! individual entries or sublists e.g.
//!
//! ```
//!    
//! ```
mod tcllist;
pub use self::tcllist::*;
