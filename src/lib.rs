//!  ringmaster is the program in NSCLDAQ that maintains knowledge
//! of and access to NSCLDAQ ringbuffers.  The main purpose of the
//! ring master is to ensure that when a client (producer or consumer)
//! exits (normally or abnormally) the bits of the ringbuffer that
//! indicate it is a client (producer/consumer ClientInformation struct)
//! are marked unused again.
//!
//! A second, but arguably even more important role the ringmaster
//! plays is to set up hoisting of data from a local ringbuffer
//! to a proxy ring in a remote system.
//!
//! This crate contains a rust port of the Tcl NSCLDAQ ringbuffer.
//! The motivation for writing it is that with NSCLDAQ running
//! increasingly within singularity containers but a need to start
//! the ringmaster at system start up, it's simpler to have a self-contained
//! ringmaster that has no dependencies on a container.
//! This is especially true in Debian-11 where the singularity
//! package seems not to be able to activate packages for init scripts.
//!
//! ##  Running the ring master:
//!
//!   The rust ring master is quite a bit more general in that it can
//! be told where the port manager is listening for connections so
//! that it can connect to it to advertise its service.  It can also
//! be told where the ring buffers actually live in anticipation
//! of, potentially, using mmap in the C++ world rather than Posix
//! shared memory which are not enumerable in Posix land.
//!   Therefore, the ring master takes the following options:
//!
//! *   --portman-port - The value of this option is the the number
//! of the port on which the port manager is listening for connections.
//! This defaults to 30000 which is the port on which NSCLDAQ normally
//! sets up the ring master.
//! *   --directory  - The directory in which the ringbuffer shared
//! memory backing files will be created. This defaults to /dev/shm
//! which is where Linux keeps its POSIX shared memory regions.
//! *   --log-file   - The file in which the ring master will make its
//! logs.
//!      
//! ## Ringmaster Application Protocol
//!
//! Clients of the ring master communicate with it via ASCII text
//! messages that are terminated by a newline.  The ringmaster responds
//! as appropriate for each request described below.        
//!
//! ### CONNECT ringname producer|consumer.n {comment string}
//!
//! Indicates a process has connected as a client to ringname either
//! as a producer or a consumer on slot n.  The comment string  is
//! not functionally used and is simple part of the log message.  The
//! client must be a local client, since shared memory is only visible
//! on the localhost.  Possible replies are:
//!
//! *   OK\r\n   - success.
//! *   ERROR reason for failure\n  - if the request failed. There are
//! several reasons this request can faie:
//!     -   The he ringname is not known to the server/
//!     -   The request was made from a remote host.
//!     -   The slot number requested does not exist in the ring.
//!     -   The pid does not, in fact, own the producer or consumer slot
//! number.
//!
//! Note that as with portmanager port allocation, the client that
//! issued the CONNECT request must remain connected to the ring master
//! and, if the connection is dropped, the effect is as if the client
//! issued a DISCONNECT request.
//!
//! On ERROR, the ringmaster will disconnect the client.
//!
//! ###  DISCONNECT  ringname producer|consumer.n pid
//!
//! Disconnects the process id specified from the ring buffer
//! named by ringname.  The thread that's monitoring the CONNECT
//! socket will exit in the near future (it's got a timed read
//! hung on the connection and will check if the read times out to
//! see if it should exit).  Possble replies are
//!
//! * OK\n - on success.  The reply is returned after the socket
//! monitoring thread has exited.
//! * ERROR - error reason string\n - on failure.  Possible failureas include
//!     -   The request was from a remote host.
//!     -   ringname did not exist.
//!     -   The slot number requested did not exist in the ringbuffer.
//!
//! Regardless of the success or failure of this request, the ringmaster
//! will disconnect the client.
//!
//! ### REGISTER ringname
//!
//!   When the ringmaster starts it will survey the target directory
//! for ringbuffers.  Files in the directory that have the appropriate
//! magic string in their first few bytes will be registered.
//! If a new ringbuffer is added by an NSCLDAQ program it will send the
//! ringmaster a REGISTER request.  The ring master will then add
//! _ringname_ to the set of ring buffers it knows about.  
//!
//! Possible replies to the client are:
//!
//! *   OK\n - on success.
//! *   ERROR reason string - The following are reasons this request can
//! fail
//!     -   The request came from a remote host.
//!     -   The ring is not known to the ringmaster.
//!
//! Once the reply is issued, the connection is dropped.
//!
//! ### UNREGISTER ringname
//!
//! When ring buffer is destroyed, this request is issued to the ringbuffer.
//! The ringmaster:
//!
//! 1.   Stops/joins all threads monitoring remaining ring clients.
//! 2.   If possible kills any client processes (in general Ringmaster
//! must be running as root to allow this).
//! 3.   Removes any knowledge of the ringbuffer from internal data
//! structures.
//!
//! Once all these actions are taken, the reply to the client is issued
//! and the connection to the client dropped.  Possible replies
//! are:
//!
//! *   OK\n  - The success comppleted successfuly.
//! *   ERROR error reason string -  The request failed.  This can happen
//! because:
//!     -   The request was from a remote host.
//!     -   The ringname was not know to the server.
//!
//! ### REMOTE ringname
//!
//! This request must not come from a local host.  It is used to set
//! up hoisting of ring data from a local ring to a remote system. Generally
//! this is done by the NSCLDAQ CRemoteAccess class to set up dataflow
//! between a ring local to this ringmaster and a proxy ring local
//! to the remote system.
//!
//! The ringmaster replies about the success or failure of the operation
//! and then forks off a subprocess that will inherit the socket
//! to actuall spew the data from the ring.  The subprocess will
//! register with the ring master as an ordinary consumer client.
//!
//! Possible replies are:
//!
//! *   OK BINARY FOLOWS  - on success once this has been emitted, the
//! client must be ready to receive data from the ring.
//! *   ERROR some error string - On failure.  In general this will
//! be one of:
//!     -   ringname was not known to the ring master.
//!     -   request was not from a remote host (we don't allow
//! hoisters to localhosts as they can, and should, just use the NSCLDAQ
//! programs ringtostdout or ringselector on pipes to access data if
//! they are not built with the NSCLDAQ libraries.
//!     -   The subprocess to hoist the data could not be started for
//! some reason.
//!
//! ### LIST
//!
//! This can be performed from local or remote hosts.  It returns
//!
//!   OK ringlist\n
//!
//! where ringlist is a Tcl (for historical reasons) formatted list that
//! describes the ring usage.  See Tcl man pages for the format of
//! Tcl lists.  The list has one element for each  known ringbuffer.
//! Each element is a two element sublist that contains
//!
//! *   The name of the ring.
//! *   A list of information about the ring in order:
//!     -  Size of the ring in bytes.
//!     -  Number of bytes that can be put into the ring before the
//! producer would stall.
//!     -  Number of consumers the ring supports.
//!     -  PID of the producer or -1 if there isn't one.
//!     -  Maximum number of bytesw that can be gotten by the furthest behind
//! consumer before it wll stall - if there are no consumers this is 0.
//!     -  Minimum number of bytes that can be gotten by the most caught up consumer
//! or 0 if there are none.
//!     -  A list of information about each consumer.  This list, which could be
//! empty has an element for each consumer.  The elements of each consumer sublist are:
//!         *  The consumer's process id
//!         *  The number of bytes of backlog for that consumer.
pub mod tcllist;
pub use tcllist::*;
pub mod rings;
pub use rings::*;
