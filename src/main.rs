pub mod tcllist;
use clap::{App, Arg};
use log::{error, info};
use nscldaq_ringbuffer::ringbuffer;
use nscldaq_ringmaster::rings::inventory;
use nscldaq_ringmaster::rings::rings;
//use portman_client;
//use simple_logging;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process;
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(target_os = "windows")]
use std::os::windows::io::*;

#[cfg(target_os = "linux")]
use std::os::unix::io::*;

// types of convenience:

type RingInventory = HashMap<String, rings::rings::RingBufferInfo>;
type SafeInventory = Arc<Mutex<RingInventory>>;
type SafeStream = Arc<Mutex<TcpStream>>;
struct RingInfo {
    name: String,
    size: usize,
    max_consumers: usize,
    min_get: usize,
    info: ringbuffer::RingStatus,
}
///
/// This holds the command line options:
///
#[derive(Debug, Clone)]
struct ProgramOptions {
    portman: u16,
    directory: String,
    log_filename: String,
}
static  SERVICE_NAME : &str = "RingMaster";
fn main() {
    let options = process_options();
    // If the ringmaster is  already running refuse to continue:

    if ringmaster_running(options.portman) {
        eprintln!("The ring master is already running/advertised");
        std::process::exit(-1);
    }
    
    simple_logging::log_to_file(&options.log_filename, log::LevelFilter::Info).unwrap();
    info!("Ringmaster Options {:#?}", options);
    info!(
        "Ringmaster doing inventory of existing rings on {}",
        options.directory
    );
    
    let ring_inventory = inventory_rings(&options.directory);

    info!("Obtaining port from portmanager...");
    let mut port_man = portman_client::Client::new(options.portman);
    let service_port: u16;
    match port_man.get(SERVICE_NAME) {
        Ok(p) => {
            service_port = p;
        }
        Err(e) => {
            error!("Unable to get a service port: {}", e.to_string());
            eprintln!(
                "Failed to get a service port from the port mangaer: {}",
                e.to_string()
            );
            process::exit(-1);
        }
    }
    info!(
        "Ringmaster will handle connections on listen port {}",
        service_port
    );

    server(service_port, options, ring_inventory);
}
///
/// Main server function.  We make a listener, and process requests
/// sent to us by clients.  Each request has its own service function.
/// We need:
///    
/// *   Our service port.
/// *   The directory so that we know where the ringbuffers are.
/// *   A mutable reference to the ringbufer inventory to operate on.
///
fn server(listen_port: u16, options: ProgramOptions, ring_inventory: RingInventory) {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", listen_port));
    let sinventory = Arc::new(Mutex::new(ring_inventory));
    if let Err(l) = listener {
        error!("Failed to listen on {} : {}", listen_port, l.to_string());
        process::exit(-1);
    }
    for client in listener.unwrap().incoming() {
        match client {
            Ok(stream) => {
                let sstream = Arc::new(Mutex::new(stream));
                let client_stream = Arc::clone(&sstream);
                let client_inventory = Arc::clone(&sinventory);
                let thread_options = options.clone();
                thread::spawn(move || {
                    handle_request(
                        client_stream,
                        thread_options.directory,
                        thread_options.portman,
                        client_inventory,
                    )
                });
            }
            Err(e) => {
                error!("Failed to accept a client: {}", e.to_string());
                process::exit(-1);
            }
        }
    }
}
/// handle a client request.
/// With the exception of CONNECT  Requests are single line entities and replies are all textual
/// as well in  a single line -- with the exception of REMOTE which is
/// wonky.
///
/// For the most part, this function will just get the request string,
/// decode it into words and use a match to dispatch the request into
/// functions specific to the request.  Those functions are expected to
/// reply to the client and, if necessary, shutdown the stream.
///
fn handle_request(client_stream: SafeStream, dir: String, portman: u16, inventory: SafeInventory) {
    // We can hang on to the stream:

    let mut stream = client_stream.lock().unwrap();

    // To read a line, make a BufReader as we've done in other.  We'll then
    // use get_request to read the line and return the busted up request
    // as a vector of strings.

    let mut pid = ringbuffer::UNUSED_ENTRY; // Records register pid

    // This map will record the CONNECT/DISCONNECT operations done
    // by the client.  Note that REMOTE must be done by a remote
    // client and hence will not every have any connections (whew):
    //
    // This is used to kill off any slot reservations the client
    // has made if it closes the connection (presumed dead).
    // clients making CONNECT/DISCONNECT are obligatd to hold the
    // connection until they're done with what they've connected to.
    //
    // The key is the path to a ringbuffer and the value is
    // a vector of Client objects which record producer/consumer
    // information for the client.
    //
    // CONNECT obviously add to this and DISCONNECT obviously removes from
    // this.

    let mut connections = HashMap::<String, Vec<rings::rings::Client>>::new();

    let mut reader = BufReader::new(stream.try_clone().unwrap());

    // Note in the loop below, fail_request will close shtudown the
    // stream which wil cause the next read_request to return an empty vector.

    loop {
        let request = read_request(&mut reader);
        info!("Request : {:#?}", request);
        if request.len() > 0 {
            match request[0].as_str() {
                "LIST" => {
                    info!("List request from {}", stream.peer_addr().unwrap());
                    if request.len() != 1 {
                        fail_request(&mut stream, "LIST does not take any parameters");
                    } else {
                        list_rings(&mut *stream, &dir, &inventory);
                    }
                }
                "REGISTER" => {
                    info!(
                        "Register request from {} (will enforce locality",
                        stream.peer_addr().unwrap()
                    );
                    if request.len() != 2 {
                        fail_request(&mut stream, "REGISTER must have only a ring name parameter");
                    } else {
                        register_ring(&mut *stream, &dir, &request[1], &inventory);
                    }
                }
                "UNREGISTER" => {
                    info!(
                        "Unregister request from {} will enforce locality",
                        stream.peer_addr().unwrap()
                    );
                    if request.len() != 2 {
                        fail_request(
                            &mut stream,
                            "UNREGISTER must have only a ring name parameter",
                        );
                    } else {
                        unregister_ring(&mut *stream, &request[1], &inventory);
                    }
                }
                "CONNECT" => {
                    info!(
                        "Connect request from {} will enforce locality",
                        stream.peer_addr().unwrap()
                    );
                    // We need at least 4
                    // In this implementation, the comment is optional.

                    if request.len() < 4 {
                        fail_request(&mut stream, "Unregister must have at least name, type, pid");
                    } else {
                        let mut comment = String::from("");
                        if request.len() == 5 {
                            comment = String::from(request[4].as_str());
                        }

                        let result = connect_client(
                            &mut *stream,
                            &request[1],
                            &request[2],
                            &request[3],
                            &comment,
                            &inventory,
                            &mut pid,
                        );
                        if let Some(client) = result {
                            record_connection(&request[1], &mut connections, client);
                        }
                    }
                }
                "DISCONNECT" => {
                    info!(
                        "Disconnect request from {} will enforce locality",
                        stream.peer_addr().unwrap()
                    );
                    // We need a ring name, a connection type and a
                    // pid.  Eventually all of those get checked for Ok-ness.

                    if request.len() != 4 {
                        fail_request(&mut stream, "Invalid request length");
                    } else {
                        let removed = disconnect_client(
                            &mut *stream,
                            &request[1],
                            &request[2],
                            &request[3],
                            &dir,
                            &connections,
                            &mut pid,
                        );
                        if let Some(client) = removed {
                            unrecord_connection(&request[1], &mut connections, client);
                        }
                    }
                }
                "REMOTE" => {
                    // Note we don't enforce locality this could be
                    // used by non NSCLDAQ programs to get a pipe from the ring.
                    info!("Remote request from {}", stream.peer_addr().unwrap());
                    if request.len() == 2 {
                        hoist_data(&mut stream, &request[1], &dir, portman, &inventory);
                        return;
                    } else {
                        fail_request(&mut stream, "Invalid request length");
                    }
                }
                _ => {
                    fail_request(&mut stream, "Invalid Request");
                }
            }
        } else {
            // Faiure... we can write a reply and shutdown but
            // the other side might have already done that:
            // These if-lets are just a fancy way to ignore Err's from
            // their functions.
            //
            fail_request(&mut stream, "Empty request");
            break;
        }
    }
    // release any slots held by oid if it's not ringbuffer::UNUSED_ENTRY.

    for (ring_name, allocations) in connections {
    	let ring_file = compute_ring_buffer_path(&dir, &ring_name);
        if let Ok(mut ringmap) = ringbuffer::RingBufferMap::new(&ring_file) {
            for a in allocations {
                match a {
                    rings::rings::Client::Consumer { slot, pid } => {
                        if ringmap.consumer(slot as usize).unwrap().get_pid() == pid {
                            if let Ok(_) = ringmap.free_consumer(slot as usize, pid) {}
                        }
                    }
                    rings::rings::Client::Producer { pid } => {
                        if ringmap.producer().get_pid() == pid {
                            if let Ok(_) = ringmap.free_producer(pid) {}
                        }
                    }
                }
            }
        }
    }
    info!("Socket service thread exiting");
}
///
/// Determine if a peer is local:
///
fn is_local_peer(stream: &TcpStream) -> bool {
    if let Ok(peer) = stream.peer_addr() {
        match peer {
            SocketAddr::V4(p) => *p.ip() == Ipv4Addr::new(127, 0, 0, 1),
            SocketAddr::V6(p) => *p.ip() == Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
        }
    } else {
        false
    }
}

fn acknowledge_client_hookup(stream: &mut TcpStream) {
    if let Ok(_) = stream.write_all(b"OK\r\n") {
        if let Ok(_) = stream.flush() {}
    }
}
///
/// produce the Arc::Mutex::ClientMonitorInfo for a producer.
/// When we return, the monitor is running and has a stream to listen to
/// as well as the way to unregister itself.
///
fn connect_producer(stream: &mut TcpStream, pid: u32) -> rings::rings::Client {
    let client = rings::rings::Client::Producer { pid };
    acknowledge_client_hookup(stream);

    client
}
///
///  Connect a consumer to a ring.
fn connect_consumer(stream: &mut TcpStream, slot: u32, pid: u32) -> rings::rings::Client {
    let client = rings::rings::Client::Consumer { pid, slot };
    acknowledge_client_hookup(stream);
    client
}
/// connect a client to a ring:
///
/// *  The client must be local.
/// *  The ring must be in our inventory.
/// *  The client must maintain a connection to the server,
/// once that connection is lost, the client registration is un-done but
/// the client is not killed (only the slot is freed).  To this end, we
/// wrap the stream clone in n Arc::Mutex::TcpStream which is feed off to
/// a monitor thread to watch for any client input or drop.
///
fn connect_client(
    stream: &mut TcpStream,
    ring: &str,
    connection_type: &str,
    pid: &str,
    _comment: &str, // Unusedi n this version.
    inventory: &SafeInventory,
    client_pid: &mut u32,
) -> Option<rings::rings::Client> {
    // Note the ring name will be encapsulated (by NSCLDAQ) in {}'s This
    // is to allow ring names with meaningful Tcl chars ('like'[] or $).
    // We're going to restrict ring names to not contain whitespace in this implementation

    let mut ring_name = String::from(ring);

    // Don't let an ill-formed ringname panic us strips the {} off the
    // ringname clients put there for the Tcl ringmaster.

    if ring_name.len() > 2 {
        ring_name = ring_name[1..ring_name.len() - 1].to_string();
    }
    info!("Connecting to '{} as {}", ring_name, connection_type);
    if !is_local_peer(stream) {
        fail_request(stream, "CONNECT must be from a local process");
    } else {
        if let Some(_) = inventory.lock().unwrap().get_mut(&ring_name) {
            // Turn this into the ring path:

            if let Ok(pid_value) = pid.parse::<u32>() {
                // The pid must match the client_pid or it's a fail --
                // unless the client pid is UNUSED_ENTRY:

                if (pid_value != *client_pid) && (*client_pid != ringbuffer::UNUSED_ENTRY) {
                    fail_request(stream, "PID spoof attempt");
                    return None;
                } else {
                    *client_pid = pid_value;
                }
                let connection = connection_type.split(".").collect::<Vec<&str>>();
                if connection.len() == 1 && connection[0] == "producer" {
                    let client_info = connect_producer(stream, pid_value);
                    return Some(client_info);
                } else if connection.len() == 2 && connection[0] == "consumer" {
                    if let Ok(slot) = connection[1].parse::<u32>() {
                        let client_info = connect_consumer(stream, slot, pid_value);
                        return Some(client_info);
                    } else {
                        fail_request(stream, "Invalid consumer slot id");
                    }
                } else {
                    fail_request(stream, "Invalid connection type");
                }
            } else {
                fail_request(stream, "Invalid process ID");
            }
        } else {
            fail_request(stream, "No such ringbuffer in inventory");
        }
    }
    None
}
// determines if a connection exists in a registration vector.

fn connection_exists(
    connection: &rings::rings::Client,
    registrations: &Vec<rings::rings::Client>,
) -> bool {
    for c in registrations {
        if connection == c {
            return true;
        }
    }
    false
}

///
///  Disconnect a client from the ring.  In this case we ensure all
/// parameters are correct:
///
/// *  The ring exists in our inventory.
/// *  The pid is an integer
/// *  The consumer type parameter is correctly formed eg. either "producer" or
/// "consumer."slot-num
/// *  There is an approprioately typed consumer with the PID identified
/// in the ring's monitorlist.
///
fn disconnect_client(
    stream: &mut TcpStream,
    ring: &str,
    connection_type: &str,
    pid: &str,
    dir: &str,
    connections: &HashMap<String, Vec<rings::rings::Client>>,
    client_pid: &mut u32,
) -> Option<rings::rings::Client> {
    // Trim the {} off the ring name:
    let mut ring_name = String::from(ring);
    if ring_name.len() > 2 {
        ring_name = ring_name[1..ring_name.len() - 1].to_string();
    }
    let filename = compute_ring_buffer_path(dir, &ring_name);
    info!("Ring buffer file {}", filename);
    if is_local_peer(&stream) {
        if let Some(registrations) = connections.get(&ring_name) {
            if let Ok(pid_num) = pid.parse::<u32>() {
                // Must match the client pid if there is one:

                if (pid_num != *client_pid) && (*client_pid != ringbuffer::UNUSED_ENTRY) {
                    fail_request(stream, "attemped PID spoof");
                } else {
                    *client_pid = pid_num;
                }

                // Producer or consumer:

                let connection_info: Vec<&str> = connection_type.split(".").collect();
                if connection_info.len() == 1 {
                    // producer.
                    if connection_info[0] != "producer" {
                        fail_request(
                            stream,
                            &format!("Connection type is invalid: {}", connection_info[0]),
                        );
                    } else {
                        let client_info = rings::rings::Client::Producer { pid: pid_num };
                        if connection_exists(&client_info, &registrations) {
                            if let Ok(_) = stream.write_all(b"OK\r\n") {}
                            if let Ok(_) = stream.flush() {}
                            if let Ok(mut map) = ringbuffer::RingBufferMap::new(&filename) {
                                if let Ok(_) = map.free_producer(pid_num) {}
                            }
                            return Some(client_info);
                        } else {
                            fail_request(
                                stream,
                                &format!("{} is not the producer of {}", pid_num, ring_name),
                            );
                        }
                    }
                } else if connection_info.len() == 2 {
                    // Consumer.

                    if connection_info[0] != "consumer" {
                        fail_request(
                            stream,
                            &format!("Connection type is invalid: {}", connection_info[0]),
                        );
                    } else {
                        if let Ok(slot_num) = connection_info[1].parse::<u32>() {
                            let client_info = rings::rings::Client::Consumer {
                                pid: pid_num,
                                slot: slot_num,
                            };
                            if connection_exists(&client_info, &registrations) {
                                if let Ok(_) = stream.write_all(b"OK\r\n") {}
                                if let Ok(_) = stream.flush() {}
                                if let Ok(mut map) = ringbuffer::RingBufferMap::new(&filename) {
                                    if let Ok(_) = map.free_consumer(slot_num as usize, pid_num) {}
                                }
                                return Some(client_info);
                            } else {
                                fail_request(
                                    stream,
                                    &format!(
                                        "{} is not a consumer on slot {} of ring {}",
                                        pid_num, slot_num, ring_name
                                    ),
                                );
                            }
                        } else {
                            fail_request(
                                stream,
                                &format!("Invalid slot number: {}", connection_info[1]),
                            );
                        }
                    }
                } else {
                    fail_request(
                        stream,
                        &format!("Invalid connection type: {}", connection_type),
                    );
                }
            } else {
                fail_request(
                    stream,
                    &format!("{} - pid must parse as an unsigned integer", pid),
                );
            }
        } else {
            fail_request(
                stream,
                &format!("{} is not a ring name registered to PID {}", ring_name, pid),
            );
        }
    } else {
        fail_request(stream, "DISCONNECT must be local");
    }
    None
}
///  unregister a ring that was deleted.
///  
///  *  The request must be local.
///  *  The ring must be in the inventory.
///  *  The file representing the ring must be in the inventory.
///  *  If the file exists (has not been deleted by the invoker),
///     it will be deleted by us.
///
/// On success "Ok\r\n" is emitted.  Regardess, the connectio is
/// closed after the request...if possible.
///
/// #### Note
///
/// If this program runs at escalated privilege, there's a bit of
/// escalated privilege in the sense that this allows a non-privileged
/// requestor to delete a ring-buffer file the requestor could not otherwise
/// delete.
///
fn unregister_ring(stream: &mut TcpStream, ring_name: &str, inventory: &SafeInventory) {
    let mut inventory = inventory.lock().unwrap();
    if is_local_peer(&stream) {
        // The inventory must contain the ring.  The file need not be present
        // as in theory there was once a ring buffer file named that if
        // it was in our inventory.

        if inventory.contains_key(ring_name) {
            if let Some(info) = inventory.get_mut(ring_name) {
                info.remove_all();
                inventory.remove(ring_name).unwrap();
            }
        }
        // It's the client's responsibility to remove the ringbuffer
        // file itself, otherwise we could be a securit hole
        // that allows people to remove rings they can't.
        // The correct sequence is :
        //   Client checks the ring file is deletable.
        //   Client checks it gets an OK response from the ring master
        //      when unregistering.
        //   Client deletes the ring.k

        // THe ring buffer does not need to be in our inventory so:

        if let Ok(_) = stream.write_all(b"OK\r\n") {}
        if let Ok(_) = stream.flush() {}
    } else {
        fail_request(stream, "UNREGISTER request only legal from local peers");
    }
}

/// register a new ring to the system:
///
/// *   The request must be local.
/// *   The ring must not already be in the inventory.
/// *   The file representing the ring must exist and be a ring buffer.
///
/// If all of that holds the ring is added to the inventory and
/// an "OK\r\n" response is emitted.  Regardless, the connection is closed.
///
fn register_ring(stream: &mut TcpStream, dir: &str, name: &str, inventory: &SafeInventory) {
    let mut inventory = inventory.lock().unwrap();
    if is_local_peer(&stream) {
        if inventory.contains_key(name) {
            if let Ok(_) = stream.write_all(b"OK\r\n") {}
            if let Ok(_) = stream.flush() {}
        } else {
            let mut full_path = PathBuf::new();
            full_path.push(dir);
            full_path.push(name);
            let full_path = String::from(full_path.to_str().unwrap());
            if let Ok(_map) = ringbuffer::RingBufferMap::new(&full_path) {
                add_ring(name, &mut inventory);
                if let Ok(_) = stream.write_all(b"OK\r\n") {}
                if let Ok(_) = stream.flush() {}
            } else {
                fail_request(stream, format!("{} is not a ringbuffer", name).as_str());
            }
        }
    } else {
        fail_request(stream, "REGISTER Must come from a local host");
    }
}
///
/// Return a vector of ring list information.
/// This is just a list of
/// list the ring information.
///    This results in a line containing OK and a Tcl list that describes
/// the rings and their usage. Each list element is a pair containing the
/// ring name, and information about it.  The information about a ring is
/// a Tcl list that contains:
///
/// *   The number of bytes in the ring.
/// *   The maximum number of bytes the producer can put in the ring without stalling.
/// *   The maximum number of consumers that can attach to the ring.,
/// *   The pid of the producer (-1 if there isn't one).
/// *   maximum number of bytes the furthest behind consumer is or 0 if there are
/// no consumers.
/// *   Maximum number of bytes th least behind consumer is or 0 if there are none.
/// *   A sublist with an element for each connected consumer which is a pair
/// containing in order:
///     *  The PID of the consumer.
///     *  The number of bytes of backlog the consumer has.
/// The stream will be closed
///
/// ##### Note
///    If the ring has disappeared, we clean, and any watches up.
fn list_rings(stream: &mut TcpStream, directory: &str, inventory: &SafeInventory) {
    let mut gone_rings = Vec::<String>::new();

    let mut inventory = inventory.lock().unwrap();

    if let Ok(_) = stream.write_all(b"OK\r\n") {
        let mut listing = tcllist::TclList::new();
        for name in inventory.keys() {
            if let Ok(ring_info) = get_ring_list_info(directory, name) {
                listing.add_element(&format_ring_info(ring_info));
            } else {
                gone_rings.push(name.to_string()); // Destroying here invalidates iterator.
            }
        }
        // our rendering of sublists means that we really need to take off the first and last characters.

        let mut listing_string = format!("{}", listing);
        if listing_string.len() >= 2 {
            listing_string = listing_string[1..listing_string.len() - 1].to_string();
        }
        if let Ok(_) = stream.write_all(format!("{}\r\n", listing_string).as_bytes()) {}
    }

    // Kill off all the rings that failed to list (they died).

    for bad_ring in gone_rings {
        if let Some(ring_info) = inventory.get_mut(&bad_ring) {
            ring_info.remove_all();
            if let Some(_) = inventory.remove(&bad_ring) {}
        }
    }
}
/// hoist data from the ring to the client.
//  - We require the RUST ring2stdout to be in the path.
//  - We run it with stdout pointed at the stream and
//    stderr, stdin off.
//  - The program options are set as follows:
//      *  --directory - is set to the directory in which we know the rings live.
//      *  --ring      - is the name of the ring passed in to the request.
//      *  --port      - is the port manager port we're using.
//      *  --comment   - Is "Hoisting to {}" where {} is replaced by the
//                       address of the request's peer.
//
fn hoist_data(
    stream: &mut TcpStream,
    ring: &str,
    dir: &str,
    portman: u16,
    inventory: &SafeInventory,
) {
    // Validate that the ring is in our ring inventory:
    // Gettin gthe bool holds the lock minmally.

    let ring_exists = inventory.lock().unwrap().contains_key(ring);
    if ring_exists {
        let process_stdout = socket_to_stdio(stream);
        let dir_arg = String::from(dir);
        let ring_arg = String::from(ring);
        let port_arg = portman.to_string();
        let comment_arg = format!("Hoisting to {}", stream.peer_addr().unwrap());

        // Output our success string and start the client program:

        match stream.write_all(b"OK BINARY FOLLOWS\r\n") {
            Ok(_) => {
                if let Err(e) = stream.flush() {
                    error!("Failed to flush BINARY FOLLOWS string {}", e);
                } else {
                    // can start the child.

                    start_hoister(process_stdout, &dir_arg, &ring_arg, &port_arg, &comment_arg);
                }
            }
            Err(e) => {
                // We just give up on error logging that.

                error!("Failed to send OK BINARY FOLLOWS  string {}", e);
            }
        }
    } else {
        fail_request(
            stream,
            format!("{} is not in the ring master's inventory", ring).as_ref(),
        );
    }
}
// Actually start the hoister:

fn start_hoister(
    proc_stdout: process::Stdio,
    rings_dir: &str,
    ring_name: &str,
    portman: &str,
    comment: &str,
) {
    let hoister = process::Command::new("ring2stdout")
        .args(&[
            "--directory",
            rings_dir,
            "--ring",
            ring_name,
            "--port",
            portman,
            "--comment",
            comment,
        ])
        .stdout(proc_stdout)
        .stderr(process::Stdio::null())
        .stdin(process::Stdio::null())
        .spawn();
    match hoister {
        Ok(_) => {
            info!("Started hoister for {} : {}", ring_name, comment);
        }
        Err(e) => {
            error!("Unable to spawn hoister process: {}", e);
        }
    }
}

/// Given a ring info struct, and it's name turns it into a Tcl list that
/// describes that ring.
///
fn format_ring_info(info: RingInfo) -> String {
    let mut result = tcllist::TclList::new();
    result.add_element(&info.name);
    let mut ring_info = tcllist::TclList::new();
    ring_info
        .add_element(&info.size.to_string())
        .add_element(&info.info.free_space.to_string())
        .add_element(&info.max_consumers.to_string());

    if info.info.producer_pid == ringbuffer::UNUSED_ENTRY {
        ring_info.add_element("-1");
    } else {
        ring_info.add_element(&info.info.producer_pid.to_string());
    }
    ring_info
        .add_element(&info.info.max_queued.to_string())
        .add_element(&info.min_get.to_string());

    // Now a sublist for each consumer:

    let mut consumer_list = tcllist::TclList::new();
    for consumer in info.info.consumer_usage {
        let mut consumer_info = tcllist::TclList::new();
        consumer_info
            .add_element(&consumer.pid.to_string())
            .add_element(&consumer.available.to_string());
        consumer_list.add_sublist(Box::new(consumer_info));
    }
    ring_info.add_sublist(Box::new(consumer_list));
    result.add_sublist(Box::new(ring_info));
    result.to_string()
}
/// get_ring_list_info
///   Given a ringbuffer - get the ring's information for the LIST - we're given the name
/// and directory string:
///
/// Ring buffer files, in theory can evaporate out from underneath us
/// so we return a result not the info:
///
fn get_ring_list_info(dir: &str, name: &str) -> Result<RingInfo, String> {
    let path = compute_ring_buffer_path(dir, name);

    match ringbuffer::RingBufferMap::new(&path) {
        Ok(mut map) => {
            let usage = map.get_usage();
            Ok(RingInfo {
                name: String::from(name),
                size: map.data_bytes(),
                max_consumers: map.max_consumers(),
                min_get: min_gettable(&usage),
                info: usage,
            })
        }
        Err(e) => Err(e),
    }
}
///
/// Return the minimum gettable bytes in a ring:
/// or 0 if there are no consumers
///
fn min_gettable(status: &ringbuffer::RingStatus) -> usize {
    let mut result = 0xffffffffffffffff as usize;
    for item in &status.consumer_usage {
        if item.available < result {
            result = item.available
        }
    }
    if result == 0xffffffffffffffff as usize {
        // no consumers likely
        result = 0;
    }
    result
}
/// Split a line of text into words:
///
fn line_to_words(line: &str) -> Vec<String> {
    let mut result = Vec::<String>::new();
    for word in line
        .to_string()
        .trim()
        .split(char::is_whitespace)
        .collect::<Vec<&str>>()
    {
        result.push(word.to_string());
    }
    result
}
///
/// read a request line from the client and break it up into
/// words which are returned as a vector.  If there's a problem
/// a zero length vector is returned...which will result in an
/// illegal request that will be failed (if possible).
///
fn read_request(reader: &mut BufReader<TcpStream>) -> Vec<String> {
    let mut result = Vec::<String>::new();
    let mut request_line = String::new();
    if let Ok(n) = reader.read_line(&mut request_line) {
        if n > 0 {
            result = line_to_words(&request_line);
        }
    }
    result
}
/// Fail a request by, if possible writing a failure
/// string to the peer and shutting down the socket.
///
///
fn fail_request(stream: &mut TcpStream, reason: &str) {
    if let Ok(_) = stream.write_all(format!("FAIL {}\r\n", reason).as_bytes()) {}
    if let Ok(_) = stream.flush() {}
    if let Ok(_) = stream.shutdown(Shutdown::Both) {}
}
/// Argument processing.  We do this with clap.  As per the main
/// comments, the options we support are:
///
/// *   --portman-port - Port we'll contact t get a new port allocated
/// for our use.
/// *   --directory   - The directory in which we look for ringbuffer
/// backing files.
/// *   --log-file the file we'll use to log what we're doing
///
fn process_options() -> ProgramOptions {
    // Define the program options to Clap and process parameters with it:

    let parser = App::new("ringmaster")
        .version("1.0")
        .author("Ron Fox")
        .about("Rust replacement for RingMaster -does not need containr")
        .arg(
            Arg::with_name("portman")
                .short("p")
                .long("portman-port")
                .value_name("PORTNUM")
                .help("Port number on which the port manager is listening for connections")
                .takes_value(true)
                .default_value("30000"),
        )
        .arg(
            Arg::with_name("directory")
                .short("d")
                .long("directory")
                .value_name("PATH")
                .help("Directory in which the ring bufffers live")
                .takes_value(true)
                .default_value("/dev/shm"),
        )
        .arg(
            Arg::with_name("log")
                .short("l")
                .long("log-file")
                .value_name("PATH")
                .help("File used to log events")
                .takes_value(true)
                .default_value("/var/log/nscldaq/ringmaster.log"),
        )
        .get_matches();

    // Initialize the result with the default values:

    let mut result = ProgramOptions {
        portman: 30000,
        directory: String::from("/dev/shm"),
        log_filename: String::from("/var/log/nscldaq/ringmaster.log"),
    };
    // Override the struct values with what we got from clap:

    // listen port

    if let Some(port) = parser.value_of("portman") {
        if let Ok(port_value) = port.parse::<u16>() {
            result.portman = port_value;
        } else {
            eprintln!("The value of --portman-port must be a 16 bit unsigned integer");
            process::exit(-1);
        }
    }
    // Ring buffer directory:

    if let Some(directory) = parser.value_of("directory") {
        // Check that the directory supplied exists:

        if fs::read_dir(directory).is_err() {
            eprintln!(
                "The value of --directory must be an existing directory was {}",
                directory
            );
            process::exit(-1);
        } else {
            result.directory = String::from(directory);
        }
    }
    // Log File:

    if let Some(file) = parser.value_of("log") {
        // We need to be able to write to the file.  the
        // only way I know how to do that is test open the file:

        let f = fs::OpenOptions::new().append(true).create(true).open(file);

        if f.is_err() {
            let error = f.err();
            eprintln!("Unable to open/create log file {} : {:?}", file, error);
        } else {
            result.log_filename = String::from(file);
        }
    }

    // Returnt he final value:

    result
}
///
/// inventory the rings in the specified directory, logging those
/// that are not and are rings.
///  The result is a hash map of RingBufferInfo indexed by ring name.
///
fn inventory_rings(directory: &str) -> RingInventory {
    let mut result = RingInventory::new();
    inventory::inventory::inventory_rings(
        directory,
        &mut |name| {
            add_ring(name, &mut result);
        },
        &mut |name| {
            log_non_ring(name);
        },
    );
    // Now that we listed the rings into our result, we need to reconstruct
    // the clients.  Unfortunately, we can't actually monitor these
    // But what we can do is allow them to actively DISCONNECT
    // without error.

    load_initial_clients(directory, &mut result);
    result
}
/// Return the filename from a full path string:
///
fn filename_from_path(name: &str) -> String {
    let p = Path::new(name).file_name().expect("Must be a filename");
    String::from(p.to_str().expect("Filename must be utf8"))
}
/// Compute the path to a ring buffer, given the directory it lives in
/// and its filename.  We need this because the inventory of ring buffers
/// only has the name of the ring buffer because that's what the
/// clients give us..but the actual ring buffer files live in a
/// specific directory.
///
fn compute_ring_buffer_path(directory: &str, filename: &str) -> String {
    let mut result_path = Path::new(directory);
    let buf = result_path.join(filename);
    result_path = buf.as_path();

    String::from(result_path.to_str().expect("BUG"))
}
///
/// load the ring inventory with the initial set of clients.
/// this is done by mapping each ring and looking at its producer
/// and consumer slots, making unmonitored clients for each entry that
/// is not unused.  This is important only if the system
/// needed a restart of the ringmaster while rings still existed.
/// Note that in the time between making the initial inventory,
/// and the enumeration of clients files could disappear so
/// we maintain a list of maps that fail and kill thos from the RingInventory.
///
fn load_initial_clients(directory: &str, inventory: &mut RingInventory) {
    let mut deleted = Vec::<String>::new();
    for (name, item) in inventory.iter_mut() {
        let full_path = compute_ring_buffer_path(directory, &name);
        if let Ok(mut ring_map) = ringbuffer::RingBufferMap::new(&full_path) {
            // Add the producer if it exists:
            let pid = ring_map.producer().get_pid();
            if pid != ringbuffer::UNUSED_ENTRY {
                info!("Adding existing producer {} to ring {}", pid, name);
                item.add_client(&Arc::new(Mutex::new(
                    nscldaq_ringmaster::rings::ClientMonitorInfo::new(
                        nscldaq_ringmaster::rings::Client::Producer { pid },
                    ),
                )));
                // now we need to look at the consumers:

                let slot_count = ring_map.max_consumers();
                for slot in 0..slot_count {
                    let c = ring_map.consumer(slot).unwrap();
                    let pid = c.get_pid();
                    if pid != ringbuffer::UNUSED_ENTRY {
                        info!("Adding existing consumer {} to ring {}", pid, name);
                        item.add_client(&Arc::new(Mutex::new(
                            nscldaq_ringmaster::rings::ClientMonitorInfo::new(
                                nscldaq_ringmaster::rings::Client::Consumer {
                                    pid,
                                    slot: slot as u32,
                                },
                            ),
                        )));
                    }
                }
            }
        } else {
            deleted.push(name.to_string()); // No longer a ringbuffer evidently.
        }
    }
    // The deleted list is the set of rings that disappeared out from under us.
    // we haven't set up any monitors so we just kill off the hashmap entry
    // and that'll kill off any dependent storage (I think).

    for name in deleted {
        inventory.remove(&name).unwrap();
    }
}

///
///  Log and add a new ring to a ringbuffer inventory:
///
fn add_ring(name: &str, list: &mut HashMap<String, rings::rings::RingBufferInfo>) {
    let filename = filename_from_path(name);
    list.insert(
        String::from(filename.as_str()),
        rings::rings::RingBufferInfo::new(name),
    );
    info!(
        "{} is a ring buffer, added to the ring buffer inventory",
        filename
    );
}
/// Log a file that is not a ringbufer:
///
fn log_non_ring(name: &str) {
    let filename = filename_from_path(name);
    info!("{} is not a ring buffer - ignored", filename);
}
//
// Record a connection in the hash of connections that's in the
// server thread:
//
fn record_connection(
    ring: &str,
    connections: &mut HashMap<String, Vec<rings::rings::Client>>,
    client: rings::rings::Client,
) {
    let mut ringname = String::from(ring);
    if ringname.len() > 2 {
        ringname = ringname[1..ringname.len() - 1].to_string();
    }
    if connections.contains_key(&ringname) {
        // Just need to add the entry to the back of the vector:

        if let Some(entry) = connections.get_mut(&ringname) {
            entry.push(client);
        }
    } else {
        let entry = vec![client];
        connections.insert(ringname, entry);
    }
}

// remove a connection record from the hash of connections that's in a
// server thread:

fn unrecord_connection(
    ring: &str,
    connections: &mut HashMap<String, Vec<rings::rings::Client>>,
    client: rings::rings::Client,
) {
    let mut ringname = String::from(ring);
    if ringname.len() > 2 {
        ringname = ringname[1..ringname.len() - 1].to_string();
    }

    if let Some(entry) = connections.get_mut(&ringname) {
        let mut found = false;
        let mut i = 0;

        for e in entry {
            match client {
                rings::rings::Client::Consumer { pid, slot } => {
                    if let rings::rings::Client::Consumer {
                        pid: epid,
                        slot: eslot,
                    } = e
                    {
                        if pid == *epid && slot == *eslot {
                            found = true;
                            break;
                        }
                    }
                }

                rings::rings::Client::Producer { pid } => {
                    if let rings::rings::Client::Producer { pid: epid } = e {
                        if pid == *epid {
                            found = true;
                            break;
                        }
                    }
                }
            }
            i = i + 1;
        }
        // If found is true, then i is the index to kill off:
        // Entry got moved in the for loop so we need to re-find it.

        if found {
            if let Some(entry) = connections.get_mut(&ringname) {
                entry.remove(i);
            }
        }
    }
}

///
/// Check to see if the ring master is already running.:
/// 
/// Returns bool true - if RingMaster is advertised on the
/// portman.
///
fn ringmaster_running(portman : u16) -> bool {
    let mut portman_client  = portman_client::Client::new(portman);
    let services = portman_client.find_by_service(SERVICE_NAME).expect("Port manager isn't running!");
    return services.len() > 0;
    
}
/// This function takes a TcpStream and turns it into
/// an process::Stdio object.  How this is done is
/// O/S specific but the result is not and allows us to
/// spawn processes with stdout set to the stream.
/// This is essential for the REMOTE operation
/// which will require us to spin off a ring2stdout process
/// To feed data from the ring to the remote requestor.
///
#[cfg(target_os = "linux")]
fn socket_to_stdio(socket: &TcpStream) -> process::Stdio {
    let sock = socket.as_raw_fd();
    unsafe { process::Stdio::from_raw_fd(sock) }
}

#[cfg(target_os = "windows")]
fn socket_to_stdio(socket: &TcpStream) -> process::Stdio {
    let sock = socket.as_raw_socket();
    unsafe { process::Stdio::from_raw_handle(sock as RawHandle) }
}

