use clap::{App, Arg};
use nscldaq_ringmaster::rings;
use nscldaq_ringmaster::tcllist;
use std::fs;
use std::io::Error;
use std::process;

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
    println!("Options {:#?}", options);
}
///
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

        let f = fs::OpenOptions::new().append(true).open(file);

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
