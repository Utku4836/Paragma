//! Checks the agent does not see: count_functions on sources it was not given.

use treesit::count_functions;

#[test]
fn hidden_free_functions() {
    assert_eq!(count_functions("fn a() {}\nfn b() -> u32 { 1 }\n"), 2);
}

#[test]
fn hidden_methods_in_impl_blocks() {
    let src = "struct S;\nimpl S {\n    fn x(&self) {}\n    pub fn y() -> Self { S }\n}\nfn z() {}\n";
    assert_eq!(count_functions(src), 3);
}

#[test]
fn hidden_functions_in_modules() {
    assert_eq!(count_functions("mod m {\n    pub fn a() {}\n    mod n {\n        fn b() {}\n    }\n}\n"), 2);
}

#[test]
fn hidden_no_functions() {
    assert_eq!(count_functions("struct S { x: u32 }\nconst N: u32 = 3;\n"), 0);
}
