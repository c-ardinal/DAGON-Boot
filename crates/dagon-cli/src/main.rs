#![forbid(unsafe_code)]

fn main() {
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("dagon - DAGON Boot command line tool\n\nUsage: dagon [--help]");
    }
}
