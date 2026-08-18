use std::{env, fs};

use translatecode_engine::backend::java::JavaBackend;
use translatecode_engine::backend::Backend;
use translatecode_engine::frontend::dart::DartFrontend;
use translatecode_engine::frontend::Frontend;

fn main() {
    let arguments = env::args().collect::<Vec<_>>();
    if arguments.len() != 3 {
        eprintln!("usage: translate_v2 <input.dart> <output.java>");
        std::process::exit(2);
    }
    let source = fs::read_to_string(&arguments[1]).expect("failed to read Dart input");
    let unit = DartFrontend.parse(&source);
    if !unit.diagnostics.is_empty() {
        for diagnostic in unit.diagnostics {
            eprintln!("{}: {}", diagnostic.code, diagnostic.message);
        }
        std::process::exit(1);
    }
    let output = JavaBackend.emit(&unit);
    fs::write(&arguments[2], output.code).expect("failed to write Java output");
}
