#[path = "util.rs"]
mod util;

extern crate serde;
extern crate serde_derive;
extern crate serde_json;

//use std::collections::HashMap;
#[path = "ord.rs"]
mod ord;

use bitcoin_scanner::Scanner;
use ord::Inscription;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct DotSats<'a> {
    p: &'a str,
    op: &'a str,
    name: &'a str,
    //name: &'a serde_json::value::RawValue,
}

pub fn main() {
    let data_dir = util::bitcoin_data_dir(bitcoin::Network::Bitcoin);

    let mut scanner = Scanner::new(data_dir);
    let tip_hash = scanner.tip_hash;
    let _tip = scanner.read_block(&tip_hash);
    // let tip_undo = scanner.read_undo(&tip_hash);

    let mut current_hash = tip_hash;

    loop {
        let record = scanner.block_index_record(&current_hash);
        let block = scanner.read_block_from_record(&record);

        //let prev_undo = scanner.read_undo(&prev_hash);
        //println!("Undo: {:?}", prev_undo.inner.len());

        let mut dotsats_count = 0;
        let mut ins_count = 0;
        block.txdata.iter().for_each(|tx| {
            if tx.input[0].witness.len() != 3 {
                return;
            };

            if let Some(ins) = Inscription::from_transaction(tx) {
                ins_count += 1;

                if ins.media() != ord::Media::Text {
                    return;
                }
                let body = ins.body().unwrap_or_default();

                let dotsats = serde_json::from_slice::<DotSats>(body).ok();
                if dotsats.is_none() {
                    return;
                }

                let dotsats = dotsats.unwrap();
                println!("sats: {dotsats:?}");
                dotsats_count += 1;
            }
        });

        println!(
            "Block height={:?} txs={:?} inscriptions={:?} dotsats={:?}",
            record.height, record.num_transactions, ins_count, dotsats_count
        );
        current_hash = record.header.prev_blockhash;
    }

    // let mut counts: HashMap<u8, usize> = HashMap::new();

    //    let count = counts.entry(0).or_insert(0);
    //    *count += 1;
    //});

    // println!("Inscriptions: {:?}", counts);
}
