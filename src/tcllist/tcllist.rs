pub mod tcllist {
    use std::fmt;
    use std::fmt::Display;

    ///
    /// The TclListElement enum is either a string
    /// or another TclList (sublist).
    ///
    enum TclListElement {
        Simple(String),
        SubList(Box<TclList>),
    }

    pub struct TclList {
        list: Vec<TclListElement>,
    }

    // Methods associated with TclList:

    impl TclList {
        ///
        ///  Creates a new, empty TclList.
        ///
        pub fn new() -> TclList {
            TclList { list: Vec::new() }
        }
        ///
        /// Adds a simple element (a string) to th end of the list.
        /// A mutable reference to the list itself is returned to support
        /// method chaining.
        ///
        pub fn add_element(&mut self, element: &str) -> &mut TclList {
            self.list
                .push(TclListElement::Simple(String::from(element)));
            self
        }
        ///
        /// Adds a constructed sublist to the end of the list.
        /// Again a mutable reference to the sublist is retunred
        /// to support method chaining.
        ///
        pub fn add_sublist(&mut self, element: Box<TclList>) -> &mut TclList {
            self.list.push(TclListElement::SubList(element));
            self
        }
    }
    // Implement trait Display for TclList so that
    // users can println! or format! it to turn it into
    // a string.
    impl Display for TclList {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let mut final_string = String::from("{");

            //each element is now either a simple string followed by
            //a space or the format of a sublist:

            for item in &self.list {
                match item {
                    TclListElement::Simple(s) => {
                        final_string = final_string + s.as_str();
                        final_string = final_string + " ";
                    }
                    TclListElement::SubList(l) => {
                        final_string = final_string + format!("{}", l).as_str();
                    }
                }
            }

            final_string = final_string + "}";
            write!(f, "{}", final_string)
        }
        // Tests for TclList.
    }
}
