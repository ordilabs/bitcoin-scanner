#![feature(thread_id_value)]
#[path = "util.rs"]
mod util;

extern crate rayon;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
use std::thread;

use rayon::prelude::*;

//use std::collections::HashMap;
#[path = "ord.rs"]
mod ord;

use bitcoin_scanner::Scanner;
use bitcoin_scanner::*;

use ord::Inscription;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
struct DotSats<'a> {
    p: &'a str,
    op: &'a str,
    name: &'a str,
    //name: &'a serde_json::value::RawValue,
}

pub fn main() {
    let network = bitcoin::Network::Bitcoin;

    let data_dir = util::bitcoin_data_dir(network);

    let mut scanner = Scanner::new(&data_dir);
    let tip_hash = scanner.tip_hash;
    let _tip = scanner.read_block(&tip_hash);
    // let tip_undo = scanner.read_undo(&tip_hash);

    //let mut current_hash = tip_hash;

    let start = std::time::Instant::now();

    scanner.load_index().unwrap();

    dbg!(start.elapsed());

    let records: Vec<BlockIndexRecord> = scanner
        .memory_index
        .clone()
        .into_values()
        .filter(|record| {
            //let record = scanner.block_index_record(hash);

            if record.height > 767430 {
                // first inscriptions
                return true;
            }
            false
        })
        .collect();

    // only 500 for testing
    let records = records[0..500].to_vec();

    let num_cpus = 2;
    rayon::ThreadPoolBuilder::new()
        .num_threads(num_cpus)
        .build_global()
        .unwrap();

    let chunk_size = (records.len() + num_cpus - 1) / num_cpus;

    dbg!(records.len(), chunk_size);

    // multithreaded code still slow probably due to contention and disk access
    // solution is to use a thread pool and a channel to send results back to main thread
    // and then write to db in main thread
    // also maybe presort records according to file and then read file once
    // send minimal amount of data across threads

    let records: Vec<String> = records
        .par_chunks(chunk_size)
        .flat_map(|chunk| {
            chunk.par_iter().map(|record| {
                let block = Scanner::read_block_from_record(&data_dir, &record);
                let height = record.clone().height.clone();

                let _names: Vec<_> = block
                    .txdata
                    .iter()
                    .filter_map(Inscription::from_transaction)
                    .filter_map(|ins| {
                        if ins.media() != ord::Media::Text {
                            return None;
                        }
                        let body = ins.body().unwrap_or_default();
                        let dotsats: DotSats = serde_json::from_slice(body).ok()?;
                        if dotsats.op.to_lowercase() != "reg" {
                            return None;
                        }
                        let thread_id = thread::current().id().as_u64();
                        //let thread_id = 0;
                        dbg!(thread_id, height, dotsats.name);
                        Some(dotsats.name.to_owned())
                    })
                    .collect();

                "hi".to_owned()
            })

            // (inscriptions, dotsats_count, ins_count)
        })
        .collect();

    if records.len() > 1 {
        return;
    }

    // scanner.memory_index.iter().for_each(|(_hash, record)| {
    //     //let record = scanner.block_index_record(hash);
    //     // dbg!(hash, record.height);
    //     if network == bitcoin::Network::Bitcoin && record.height < 767430 {
    //         // first inscriptions
    //         return;
    //     }
    //     let block = Scanner::read_block_from_record(&data_dir, &record);

    //     //let prev_undo = scanner.read_undo(&prev_hash);
    //     //println!("Undo: {:?}", prev_undo.inner.len());

    //     let dotsats_count = 0;
    //     let ins_count = 0;

    //     let inscriptions: Vec<String> = block
    //         .txdata
    //         .iter()
    //         .flat_map(|tx| -> Option<String> {
    //             if tx.input[0].witness.len() != 3 {
    //                 return None;
    //             };

    //             let name = if let Some(ins) = Inscription::from_transaction(tx) {
    //                 //ins_count += 1;

    //                 if ins.media() != ord::Media::Text {
    //                     return None;
    //                 }
    //                 let body = ins.body().unwrap_or_default();

    //                 let dotsats = serde_json::from_slice::<DotSats>(body).ok();
    //                 if dotsats.is_none() {
    //                     return None;
    //                 }

    //                 let dotsats = dotsats.unwrap().clone();
    //                 if dotsats.op.to_lowercase() != "reg" {
    //                     return None;
    //                 }
    //                 println!("sats: {dotsats:?}");
    //                 let name = dotsats.name.to_owned();
    //                 Some(name)
    //                 //dotsats_count += 1;
    //             } else {
    //                 None
    //             };
    //             name
    //         })
    //         .collect();

    //     println!(
    //         "Block height={:?} txs={:?} inscriptions={:?} dotsats={:?}",
    //         record.height, record.num_transactions, ins_count, dotsats_count
    //     );

    //     dbg!(inscriptions.len());
    // });

    // loop {
    //     let record = scanner.block_index_record(&current_hash);
    //     let block = Scanner::read_block_from_record(&data_dir, &record);

    //     //let prev_undo = scanner.read_undo(&prev_hash);
    //     //println!("Undo: {:?}", prev_undo.inner.len());

    //     let mut dotsats_count = 0;
    //     let mut ins_count = 0;
    //     block.txdata.iter().for_each(|tx| {
    //         if tx.input[0].witness.len() != 3 {
    //             return;
    //         };

    //         if let Some(ins) = Inscription::from_transaction(tx) {
    //             ins_count += 1;

    //             if ins.media() != ord::Media::Text {
    //                 return;
    //             }
    //             let body = ins.body().unwrap_or_default();

    //             let dotsats = serde_json::from_slice::<DotSats>(body).ok();
    //             if dotsats.is_none() {
    //                 return;
    //             }

    //             let dotsats = dotsats.unwrap();
    //             println!("sats: {dotsats:?}");
    //             dotsats_count += 1;
    //         }
    //     });

    //     println!(
    //         "Block height={:?} txs={:?} inscriptions={:?} dotsats={:?}",
    //         record.height, record.num_transactions, ins_count, dotsats_count
    //     );
    //     current_hash = record.header.prev_blockhash;
    // }

    // let mut counts: HashMap<u8, usize> = HashMap::new();

    //    let count = counts.entry(0).or_insert(0);
    //    *count += 1;
    //});

    // println!("Inscriptions: {:?}", counts);
}
