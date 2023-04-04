#![cfg(feature = "binary")]

use clap::Parser;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    #[arg(short, long)]
    name: String,

    /// Number of times to greet
    #[arg(short, long, default_value_t = 1)]
    count: u8,
}

pub fn main() {
    let args = Args::parse();
    a = 1u32 + 1usize;
    for _ in 0..args.count {
        println!("Hello {}!", args.name)
    }
}
