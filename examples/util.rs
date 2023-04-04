use std::path::PathBuf;
extern crate directories;
use directories::BaseDirs;

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

    data_dir
}

#[allow(dead_code)]
pub fn main() {
    print!("Hello, world!");
}
