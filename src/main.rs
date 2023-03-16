use std::env;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} /path/to/rev*.dat", args[0]);
        std::process::exit(1);
    }

    let filepath = &args[1];
    let mut file = File::open(filepath)?;

    parse_rev_file(&mut file)?;

    Ok(())
}

fn parse_rev_file(file: &mut File) -> io::Result<()> {
    // Your parsing logic will be implemented here
    unimplemented!();
}
