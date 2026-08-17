use std::env;
use std::fs;
use tree_sitter::Parser;

fn main() {
    let path = env::args().nth(1).expect("usage: parse_dart <file.dart>");
    let source = fs::read_to_string(path).expect("failed to read Dart source");
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_dart::LANGUAGE.into())
        .expect("failed to load Dart grammar");
    let tree = parser.parse(&source, None).expect("Dart parser returned no tree");
    println!("{}", tree.root_node().to_sexp());
    if tree.root_node().has_error() {
        std::process::exit(1);
    }
}
