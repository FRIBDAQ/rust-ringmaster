//!
//! The tcllist module provides the ability to build and
//! format simple Tcl lists.  The lists are simple in the sense
//! that
//! *   The main list is surrounded by {}.
//! *   It is assumed each element of the list requires no quoting execpt:
//! *   Sublists are surrounded by {} as well.
//!
//! Normally you'd create a list and then add to it.   You can add either
//! individual entries or sublists.  Formatting is supported e.g.
//!
//! ```
//!   use nscldaq_ringmaster::tcllist::tcllist::tcllist::*;
//!
//!   let mut top_list = TclList::new();   
//!   let mut inner_list = TclList::new();
//!   top_list.add_element("Text");
//!   inner_list
//!     .add_element("element")
//!     .add_element("another");
//!   top_list.add_sublist(Box::new(inner_list));
//!
//!   println!("{}", top_list);
//! ```
//!
//! will produce something like:
//!
//!   {Text {element another}}
//!
//! note that element of a list that add to the list
//! will in general return the list itself to suppor chaining  as
//! shown in the code that builds ```inner_list```.
pub mod tcllist;
pub use self::tcllist::*;
