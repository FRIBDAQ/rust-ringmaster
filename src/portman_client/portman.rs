pub mod portman {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use whoami;
    /// Error reporting is via one of these enumerated constant in a Result Err
    /// The function to_string is defined on the enum to convert enum elements
    /// into a human readable string:
    ///
    pub enum Error {
        ConnectionFailed,
        Unimplemented,
        AllocationFailed,
        ConnectionLost,
        RequestDenied,
        UnanticipatedReply,
    }
    impl Error {
        /// returns a human readable string that describes the
        /// error we are portraying.
        ///
        pub fn to_string(&self) -> String {
            match self {
                ConnectionFailed => String::from("Connection to port manager failed"),
                Unimplemented => String::from("This operation is not yet implemented"),
                AllocationFailed => String::from("Failed to allocate a port from the manager"),
                ConnectionLost => String::from("connection with server lost"),
                RequestDenied => String::from("Server returned a failure on the request."),
                UnanticipatedReply => {
                    String::from("The server reply was not an anticipated string")
                }
            }
        }
    }
    ///
    /// This struct describes a service advertisement:
    ///
    #[derive(Debug, Clone)]
    pub struct Allocation {
        pub port: u16,
        pub service_name: String,
        pub user_name: String,
    }

    ///
    /// Object through which to communicate with the port manager.
    /// We support the following operations:
    ///
    /// *   get - allocate and advertise a port/service.
    /// *   list - list all port allocations.
    /// *   find_by_service - lists services that match a name
    /// *   find_by_user    - lists services a user advertises
    /// *   find_my_service - Locates, by name a service I advertise.
    /// *   find_exact      - find a service by user/service_name.
    ///
    /// Note that at present we only support operations with the local
    /// port manager as remote port manager operations cannot allocate ports
    ///
    pub struct Client {
        port: u16,
        connection: Option<TcpStream>,
    }

    impl Client {
        // If necessary, create the connection.
        //
        fn make_connection(&mut self) -> Result<TcpStream, Error> {
            if self.connection.as_ref().is_some() {
                Ok(self
                    .connection
                    .as_ref()
                    .expect("should be some")
                    .try_clone()
                    .unwrap())
            } else {
                let address = format!("127.0.0.1:{}", self.port);
                match TcpStream::connect(&address) {
                    Ok(socket) => {
                        self.connection = Some(socket);
                        Ok(self
                            .connection
                            .as_ref()
                            .expect("should be some")
                            .try_clone()
                            .unwrap())
                    }
                    Err(reason) => Err(Error::ConnectionFailed),
                }
            }
        }
        // Get a reply from the server.  Note that we are not actually
        // an object operation.  If read the read succeeds, the
        // reply is broken in to words.  If the first word is FAIL,
        // an RequestDenied error is returned if OK, then
        // the remaining words are returned as they represent the
        // server response.
        //
        fn get_reply(socket: &mut TcpStream) -> Result<Vec<String>, Error> {
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let mut reply = String::new();
            if reader.read_line(&mut reply).unwrap() > 0 {
                let words: Vec<&str> = reply.split(" ").collect();
                match words[0] {
                    "OK" => {
                        let mut result = Vec::<String>::new();
                        if words.len() > 1 {
                            // Might just be Ok.
                            for w in &words[1..] {
                                result.push(String::from(*w));
                            }
                        }
                        Ok(result)
                    }
                    "FAIL" => Err(Error::RequestDenied),
                    _ => Err(Error::UnanticipatedReply),
                }
            } else {
                Err(Error::ConnectionLost)
            }
        }
        // Get the lines from the server that contain the list of port allocations.
        // These are triplets of the form
        //
        //    port-num service-name advertising-user
        //
        // It's still possible for errors to occur (e.g. the server dies in the middle of)
        // writing these lines.
        //
        fn get_allocations(socket: &mut TcpStream, n: usize) -> Result<Vec<Allocation>, Error> {
            // Easier to read lines if the socket get wrapped up in a BufReader:

            let mut result: Vec<Allocation> = Vec::new();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            for i in 0..n {
                let mut allocation_string = String::new();
                if let Ok(size) = reader.read_line(&mut allocation_string) {
                    if size > 0 {
                        let words: Vec<&str> = allocation_string.split(" ").collect();
                        if words.len() == 3 {
                            let service = String::from(words[1]);
                            let user = String::from(words[2]);
                            if let Ok(port) = String::from(words[0]).parse::<u16>() {
                                result.push(Allocation {
                                    port: port,
                                    service_name: service,
                                    user_name: user,
                                });
                            } else {
                                return Err(Error::UnanticipatedReply);
                            }
                        } else {
                            return Err(Error::UnanticipatedReply);
                        }
                    } else {
                        return Err(Error::ConnectionLost);
                    }
                } else {
                    return Err(Error::ConnectionLost);
                }
            }
            return Ok(result);
        }

        ///
        /// Create a client object.  The client is not
        /// initially connected to the server.  The connection to the
        /// server happens on the first operation  and is then maintained
        /// until the object is dropped (we implement the Drop trait
        /// in order to be sure the stream is properly closed).
        /// Note that if a program is allocating a port is must maintain
        /// the object as long as the lifetime of the service else
        /// the advertisement of the port will be dropped and the port
        /// freed.
        ///
        pub fn new(port: u16) -> Client {
            Client {
                port: port,
                connection: None,
            }
        }

        ///
        /// Ask the manager to allocate a port and advertise it as a service.
        /// This is done by sending the message:: GIMME service username
        /// on success we'll get back OK number
        /// On failure the first word of ther response will be fail and
        /// our socket will get dropped.
        ///
        /// The Ok branch of the result is the port number that was allocated.
        ///
        pub fn get(&mut self, service_name: &str) -> Result<u16, Error> {
            match self.make_connection() {
                Err(e) => Err(e),
                Ok(mut socket) => {
                    let me = whoami::username();
                    let request = format!("GIMME {} {}\n", service_name, me);
                    // Send the request
                    if let Err(e) = socket.write_all(request.as_bytes()) {
                        return Err(Error::ConnectionLost);
                    }

                    if let Err(e) = socket.flush() {
                        return Err(Error::ConnectionLost);
                    }
                    //
                    // Get/processcargo  the reply:
                    //
                    match Self::get_reply(&mut socket) {
                        Ok(port) => {
                            // port must be a one element array containing the
                            // port number:
                            if port.len() == 1 {
                                let parsed_port = port[0].parse::<u16>();
                                match parsed_port {
                                    Ok(num) => Ok(num),
                                    Err(_) => Err(Error::UnanticipatedReply),
                                }
                            } else {
                                Err(Error::UnanticipatedReply)
                            }
                        }

                        Err(reason) => Err(reason),
                    }
                }
            }
        }
        ///
        /// List all the port allocations.  On success, thesse are returned as a
        /// vector of Allocation objects which the user can interrogate to
        /// determine if they contain what's needed.  Note as well
        /// that this function is called by all of the methods that search
        /// for specific allocation (sets) as the port manager can only
        /// return all allocations.  Any filtering must be done client side.
        ///
        pub fn list(&mut self) -> Result<Vec<Allocation>, Error> {
            match self.make_connection() {
                Err(e) => Err(e),
                Ok(mut socket) => {
                    // Format and send the message:

                    if let Err(e) = socket.write_all(b"LIST\n") {
                        return Err(Error::ConnectionLost);
                    }
                    if let Err(e) = socket.flush() {
                        return Err(Error::ConnectionLost);
                    }
                    // The first reply word will contain the number of service lines to follow:

                    match Self::get_reply(&mut socket) {
                        Ok(tail) => {
                            if tail.len() == 1 {
                                let num_lines = tail[0].parse::<usize>();
                                match num_lines {
                                    Ok(n) => Err(Error::Unimplemented),
                                    Err(_) => Err(Error::UnanticipatedReply),
                                }
                            } else {
                                Err(Error::UnanticipatedReply)
                            }
                        }
                        Err(reason) => Err(reason),
                    }
                }
            }
        }
        ///
        /// Find a service advertisement by service name. Note that since this is not
        /// qualified by the user name, more than one result may be returned on success.
        ///
        fn find_by_service(&mut self, service_name: &str) -> Result<Vec<Allocation>, Error> {
            match self.list() {
                Ok(all_services) => {
                    let result: Vec<Allocation> = all_services
                        .into_iter()
                        .filter(|item| item.service_name == service_name)
                        .collect::<Vec<Allocation>>();

                    Ok(result)
                }
                Err(e) => Err(e),
            }
        }
    }
}
