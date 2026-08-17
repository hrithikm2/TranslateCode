use std::env;
use std::io::{self, Read};
use translatecode_engine::translate_by_id;

fn main() {
    let args = env::args().collect::<Vec<_>>();
    if args.len() != 3 {
        eprintln!("usage: translate <source-language-id> <target-language-id>");
        std::process::exit(2);
    }
    let from = args[1].parse::<u32>().expect("invalid source language id");
    let to = args[2].parse::<u32>().expect("invalid target language id");
    let mut source = String::new();
    io::stdin().read_to_string(&mut source).expect("failed to read source");
    print!("{}", translate_by_id(&source, from, to));
}
