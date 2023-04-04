use std::path::PathBuf;
extern crate directories;
use directories::BaseDirs;
use std::fs::File;
use std::io::{self, BufRead};

pub fn bitcoin_data_dir(network: bitcoin::Network) -> PathBuf {
    let mut data_dir: PathBuf = BaseDirs::new().unwrap().data_dir().into();

    match std::env::consts::OS {
        "windows" | "macos" => {
            data_dir.push("Bitcoin");
        }
        _ => {
            data_dir.push(".bitcoin");
        }
    };

    match network {
        bitcoin::Network::Bitcoin => {}
        bitcoin::Network::Testnet => {
            data_dir.push("testnet3");
        }
        bitcoin::Network::Regtest => {
            data_dir.push("regtest");
        }
        bitcoin::Network::Signet => {
            data_dir.push("signet");
        }
    };

    // Check if bitcoin.conf contains a datadir setting and if so, use that instead.
    let bitcoin_conf_path = data_dir.join("bitcoin.conf");

    if bitcoin_conf_path.exists() {
        let datadir = find_datadir(bitcoin_conf_path).unwrap();
        if datadir.is_some() {
            return datadir.unwrap();
        }
    }

    return data_dir;
}

#[allow(dead_code)]
pub fn main() {
    print!("Hello, world!");
}

fn find_datadir(bitcoin_conf_path: PathBuf) -> io::Result<Option<PathBuf>> {
    let file = File::open(bitcoin_conf_path)?;
    let reader = io::BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let trimmed_line = line.trim();

        if trimmed_line.starts_with("datadir=") {
            let datadir = trimmed_line.strip_prefix("datadir=").unwrap();
            return Ok(Some(PathBuf::from(datadir)));
        }
    }

    Ok(None)
}
