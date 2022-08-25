use clap::{App, Arg};
use log::{error, info, trace, warn};
use nscldaq_ringbuffer::ringbuffer;
use nscldaq_ringmaster::portman_client::portman::*;
use nscldaq_ringmaster::rings::inventory;
use nscldaq_ringmaster::rings::rings;
use nscldaq_ringmaster::tcllist;
use simple_logging;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io::Error;
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::process;
use std::sync::{Arc, Mutex};

// types of convenience:

type RingInventory = HashMap<String, rings::rings::RingBufferInfo>;

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

fn main() {
    let options = process_options();
    simple_logging::log_to_file(&options.log_filename, log::LevelFilter::Info).unwrap();
    info!("Ringmaster Options {:#?}", options);
    info!(
        "Ringmaster doing inventory of existing rings on {}",
        options.directory
    );
    let mut ring_inventory = inventory_rings(&options.directory);

    info!("Obtaining port from portmanager...");
    let mut port_man = portman::Client::new(options.portman);
    let service_port: u16;
    match port_man.get("RingMaster") {
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

    server(service_port, &options.directory, &mut ring_inventory);
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
fn server(listen_port: u16, ring_directory: &str, ring_inventory: &mut RingInventory) {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", listen_port));
    if let Err(l) = listener {
        error!("Failed to listen on {} : {}", listen_port, l.to_string());
        process::exit(-1);
    }
    for client in listener.unwrap().incoming() {
        match client {
            Ok(mut stream) => {
                handle_request(stream, ring_directory, ring_inventory);
            }
            Err(e) => {
                error!("Failed to accept a client: {}", e.to_string());
                process::exit(-1);
            }
        }
    }
}
/// handle a client request.
/// With the exception of CONNECT, all of the requests are state-less,
/// by that I mean that after the request is completed, the connection is
/// dropped.  Requests are single line entities and replies are all textual
/// as well in  a single line -- with the exception of REMOTE which is
/// wonky.
///
/// For the most part, this function will just get the request string,
/// decode it into words and use a match to dispatch the request into
/// functions specific to the request.  Those functions are expected to
/// reply to the client and, if necessary, shutdown the stream.
///
fn handle_request(mut stream: TcpStream, dir: &str, inventory: &mut RingInventory) {
    // To read a line, make a BufReader as we've done in other.  We'll then
    // use get_request to read the line and return the busted up request
    // as a vector of strings.

    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let request = read_request(&mut reader);
    if request.len() > 0 {
        match request[0].as_str() {
            "LIST" => {
                info!("List request from {}", stream.peer_addr().unwrap());
                list_rings(stream, dir, inventory);
            }
            _ => {
                fail_request(stream, "Invalid Request");
            }
        }
    } else {
        // Faiure... we can write a reply and shutdown but
        // the other side might have already done that:
        // These if-lets are just a fancy way to ignore Err's from
        // their functions.
        //
        fail_request(stream, "Empty request");
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
fn list_rings(mut stream: TcpStream, directory: &str, inventory: &mut RingInventory) {
    // stub
    // We're ignoring failures because eventually we'll shutdown the stream anyway.
    if let Ok(_) = stream.write_all(b"Ok\n") {
        let mut listing = tcllist::TclList::new();
        for name in inventory.keys() {
            if let Ok(ring_info) = get_ring_list_info(directory, name) {
                listing.add_element(&format_ring_info(ring_info));
            }
        }
        if let Ok(_) = stream.write_all(format!("{}\n", listing).as_bytes()) {}
    }
    if let Ok(_) = stream.shutdown(Shutdown::Both) {}
}
/// Given a ring info struct, and it's name turns it into a Tcl list that
/// describes that ring.
///
fn format_ring_info(info: RingInfo) -> String {
    let mut result = tcllist::TclList::new();
    result
        .add_element(&info.name)
        .add_element(&info.size.to_string())
        .add_element(&info.info.free_space.to_string())
        .add_element(&info.max_consumers.to_string());

    if info.info.producer_pid == ringbuffer::UNUSED_ENTRY {
        result.add_element("-1");
    } else {
        result.add_element(&info.info.producer_pid.to_string());
    }
    result
        .add_element(&info.info.max_queued.to_string())
        .add_element(&info.min_get.to_string());

    // Now a sublist for each consumer:

    for consumer in info.info.consumer_usage {
        let mut consumer_info = tcllist::TclList::new();
        consumer_info
            .add_element(&consumer.pid.to_string())
            .add_element(&consumer.available.to_string());
        result.add_sublist(Box::new(consumer_info));
    }
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
    let mut result = usize::MAX;
    for item in &status.consumer_usage {
        if item.available < result {
            result = item.available
        }
    }
    if result == usize::MAX {
        // no consumers likely
        result = 0;
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
    if let Ok(len) = reader.read_line(&mut request_line) {
        // Got a line-- maybe:
        if len > 0 {
            request_line.trim(); // Kills off the trailing \n too.
            for word in request_line
                .split(char::is_whitespace)
                .collect::<Vec<&str>>()
            {
                result.push(String::from(word));
            }
        }
    }
    result
}
/// Fail a request by, if possible writing a failure
/// string to the peer and shutting down the socket.
///
///
fn fail_request(mut stream: TcpStream, reason: &str) {
    if let Ok(_) = stream.write_all(format!("FAIL {}\n", reason).as_bytes()) {}
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
