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
                    final_string = final_string + " ";
                }
            }
        }

        final_string = final_string + "}";
        write!(f, "{}", final_string)
    }
}
// Tests for TclList.
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new() {
        // empty TclList formats as {}

        let l = TclList::new();
        assert_eq!("{}", format!("{}", l));
    }
    #[test]
    fn simple_1() {
        // format a list with one simple element:

        let mut l = TclList::new();
        l.add_element("String");
        assert_eq!("{String }", format!("{}", l));
    }
    #[test]
    fn simple_n() {
        //format a list with a few simples:

        let mut l = TclList::new();
        l.add_element("1")
            .add_element("2")
            .add_element("3")
            .add_element("4");

        assert_eq!("{1 2 3 4 }", format!("{}", l));
    }
    #[test]
    fn sublist_1() {
        // list with a simple sublist:

        let mut l = TclList::new();
        let mut sublist = TclList::new();
        sublist.add_element("a").add_element("b");
        l.add_sublist(Box::new(sublist));

        assert_eq!("{{a b } }", format!("{}", l));
    }
    #[test]
    fn sublist_2() {
        // list with two sublists.
        let mut l = TclList::new();
        let mut sub1 = TclList::new();
        let mut sub2 = TclList::new();
        sub1.add_element("1").add_element("2").add_element("3");
        sub2.add_element("a").add_element("b").add_element("c");
        l.add_sublist(Box::new(sub1)).add_sublist(Box::new(sub2));
        assert_eq!("{{1 2 3 } {a b c } }", format!("{}", l));
    }
    #[test]
    fn mixed() {
        // Mixed simple and sublist list:

        let mut l = TclList::new();
        let mut sub1 = TclList::new();
        let mut sub2 = TclList::new();
        sub1.add_element("1").add_element("2").add_element("3");
        sub2.add_element("a").add_element("b").add_element("c");
        l.add_element("outer1")
            .add_sublist(Box::new(sub1))
            .add_element("outer2")
            .add_sublist(Box::new(sub2))
            .add_element("final");
        assert_eq!("{outer1 {1 2 3 } outer2 {a b c } final }", format!("{}", l));
    }
    #[test]
    fn nested() {
        let mut l = TclList::new();
        let mut sub1 = TclList::new();
        let mut sub2 = TclList::new();
        sub2.add_element("a").add_element("b").add_element("c");
        sub1.add_element("1")
            .add_sublist(Box::new(sub2))
            .add_element("2")
            .add_element("3");
        l.add_element("whoo")
            .add_sublist(Box::new(sub1))
            .add_element("hoo");
        assert_eq!("{whoo {1 {a b c } 2 3 } hoo }", format!("{}", l));
    }
}
