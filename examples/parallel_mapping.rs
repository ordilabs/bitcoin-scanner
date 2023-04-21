#[path = "util.rs"]
mod util;

extern crate serde;
extern crate serde_derive;
extern crate serde_json;

extern crate ord_labs;
use ord_labs::*;

use bitcoin::opcodes::all::*;
use bitcoin::{BlockHash, Transaction};
use bitcoin_scanner::db::{InscriptionRecord, SatsNameRecord, DB};
use bitcoin_scanner::{BlockIndexRecord, Scanner, TxInUndo};
use deadpool_postgres::{Config, Pool, PoolConfig};
use num_cpus;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use tokio::runtime;
use tokio_postgres::NoTls;
use std::sync::{Arc, Mutex};
use async_std::task::block_on;

#[derive(Serialize, Deserialize, Debug)]
struct DotSats<'a> {
    p: &'a str,
    op: &'a str,
    name: &'a str,
}

async fn create_db_pool() -> Pool {
    let mut cfg = Config::new();
    cfg.dbname = Some("ordscanner".to_owned());
    cfg.user = Some("orduser".to_owned());
    cfg.password = Some("testtest".to_owned());
    cfg.host = Some("localhost".to_owned());
    cfg.port = Some(5432);

    let mut pool_config = PoolConfig::new(100);
    pool_config.max_size = num_cpus::get();
    // pool_config.recycling_method = RecyclingMethod::Fast;
    cfg.pool = Some(pool_config);

    cfg.create_pool(None, NoTls).unwrap()
}

fn index_parser(tx: mpsc::Sender<(Arc<Scanner>, Vec<BlockIndexRecord>)>, scanner: Arc<Scanner>, pool: Arc<Pool>) {
    println!("IndexParser: Started!");
    let mut current_hash = scanner.tip_hash;

    let batch_size = 100;
    let batch: Vec<BlockIndexRecord> = vec![];

    loop {
        let record = scanner.block_index_record(&current_hash);
        batch.push(record);

        // We can stop scanning once we have parsed the block containing inscription #0, i.e. block
        // height 767430.
        let stop_block =
            BlockHash::from_str("000000000000000000029730547464F056F8B6E2E0A02EAF69C24389983A04F5")
                .unwrap();
        if current_hash == stop_block {
            // Send last batch.
            tx.send((scanner, batch)).unwrap();
            println!("IndexParser: Finished!");
            break;
        }

        current_hash = record.header.prev_blockhash;

        if batch.len() == batch_size {
            tx.send((scanner, batch)).unwrap();
            batch = vec![];
        }
    }
}

fn worker(id: usize, shared_rx: &Arc<Mutex<Receiver<(Arc<Scanner>, Vec<BlockIndexRecord>)>>>, pool: &Pool) {
    let rx = shared_rx.lock().unwrap();
    while let Ok((scanner, batch)) = rx.recv() {
        for record in batch {
            let block = scanner.read_block_from_record(&record);
            let undo = scanner.read_undo(&record.header.block_hash());

            // Keeping the current input count on the block we are parsing in order
            // to look up the corresponding spent UTXO from the rev file to get the
            // pubkey that was spent from.
            let mut blk_tx_count = 0;

            // Count inscriptions found in each block.
            let mut ins_block_count = 0;
            let mut sats_name_count = 0;

            block.txdata.iter().enumerate().for_each(|(tx_i, tx)| {
                if let Some(ins) = Inscription::from_transaction(tx) {
                    let tx_ins = &undo.inner[blk_tx_count].0;
                    let spk = tx_ins[0].script.to_bytes();

                    // Only Inscriptions at index 0 are currently recognized so we
                    // can hardcode it here for now.
                    let i = 0;

                    let mut possible_inscriber: [u8; 32] = [0; 32];
                    let mut inscribers: Vec<[u8; 32]> = vec![];

                    for w in tx.input[i].witness.to_vec() {
                        // Length matches to a x-only pubkey. We save it to check
                        // if the next opcode is a checksigadd.
                        if w.len() == 32 {
                            possible_inscriber = w.clone().try_into().unwrap();
                        }

                        // If we get a checksigadd we can put the possible
                        // inscriber in to the DB.
                        if w.len() == 1 && w[0] == OP_CHECKSIGADD.to_u8() {
                            if possible_inscriber != [0; 32] {
                                inscribers.push(possible_inscriber);
                                possible_inscriber = [0; 32];
                            }
                        }
                    }

                    let address = bitcoin::Address::from_script(
                        &tx.output[i].script_pubkey,
                        bitcoin::Network::Bitcoin,
                    )
                    .unwrap();
                    let txid: [u8; 32] = *tx.txid().to_raw_hash().as_ref();
                    let block_hash: [u8; 32] = *block.header.block_hash().as_ref();
                    let short_input_id = calculate_short_input_id(
                        record.height,
                        tx_i.try_into().unwrap(),
                        i.try_into().unwrap(),
                    );

                    let insert_rec = InscriptionRecord {
                        _id: 0,
                        commit_output_script: spk,
                        txid: txid,
                        index: i,
                        genesis_inscribers: inscribers,
                        genesis_amount: tx.output[i].value,
                        address: address.to_string(),
                        content_length: ins.body().unwrap_or(&Vec::new()).len(),
                        content_type: ins.content_type().unwrap_or("").to_string(),
                        genesis_block_hash: block_hash,
                        genesis_fee: calculate_fee(tx, tx_ins),
                        genesis_height: record.height,
                        short_input_id: short_input_id,
                    };

                    ins_block_count += 1;

                    let maybe_sats_name = identify_sats_name(ins);

                    block_on(async {
                        let db = pool.get().await.unwrap();
                        let ord_db = DB::setup_with_client(false, db).unwrap();
                        let ins_res = ord_db.insert_inscription(&insert_rec).await;

                        if maybe_sats_name.is_ok() {
                            sats_name_count += 1;
                            let sats_name_rec = SatsNameRecord {
                                _id: 0,
                                inscription_record_id: ins_res.unwrap(),
                                short_input_id: short_input_id,
                                name: maybe_sats_name.unwrap(),
                            };

                            let _res = ord_db.insert_sats_name(&sats_name_rec).await;
                        }
                    });
                }

                blk_tx_count += 1;
            });

            println!(
                "Worker {}: Processed block {:?}, inserted {} inscription records and {} sats names.",
                id, record.height, ins_block_count, sats_name_count
            );
        }
    }
}

pub fn main() {
    let data_dir = util::bitcoin_data_dir(bitcoin::Network::Bitcoin);
    let mut scanner = Scanner::new(data_dir);
    let scanner = Arc::new(scanner);
    // just for general reset
    let _db = DB::setup(true).unwrap();

    let (tx, rx) = mpsc::channel();

    let pool = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(create_db_pool());

    let shared_pool = Arc::new(pool);

    let index_parser_handle = std::thread::spawn(move || {
        index_parser(tx, scanner, shared_pool);
    });

    let shared_rx = Arc::new(Mutex::new(rx));

    rayon::scope(|scope| {
        let num_consumers = num_cpus::get();

        for id in 0..num_consumers {
            let shared_rx = shared_rx.clone();
            let pool = Arc::clone(&shared_pool);

            scope.spawn(move |_| {
                worker(id, &shared_rx, &*pool);
            });
        }
    });

    index_parser_handle.join().unwrap();
}

fn calculate_fee(tx: &Transaction, tx_undo: &Vec<TxInUndo>) -> u64 {
    let input_sum: u64 = tx_undo.iter().map(|input| input.amount).sum();

    let output_sum: u64 = tx.output.iter().map(|output| output.value).sum();

    input_sum - output_sum
}

fn calculate_short_input_id(block_height: u32, transaction_index: u32, input_index: u16) -> i64 {
    (((block_height as i64) << 40) | ((transaction_index as i64) << 16) | (input_index as i64)) * -1
}

fn identify_sats_name(ins: Inscription) -> Result<String, ()> {
    if ins.media() != Media::Text {
        return Err(());
    }

    let body = ins.body().unwrap_or_default();

    let dotsats = serde_json::from_slice::<DotSats>(body).ok();
    if dotsats.is_none() {
        // Check if this is a simple registration in the body instead:
        // See: https://docs.sats.id/sats-names/protocol-spec#simple-registration
        match std::str::from_utf8(body) {
            Ok(s) => {
                let result = parse_sats_name_body(s);
                match result {
                    Some(s) => return Ok(s),
                    None => return Err(()),
                }
            }
            Err(_e) => return Err(()),
        }
    }

    let dotsats = dotsats.unwrap();

    if dotsats.p != "sns" || dotsats.op != "reg" {
        return Err(());
    }

    // Parse name and ensure all the specs are followed
    // 1. Turn the string into lowercase
    // 2. Delete everything after the first whitespace or newline (\n)
    // 3. Trim all whitespace and newlines
    // 4. Validate that there is only one period (.) in the name
    // 5. Validate that the string ends with .sats
    // See: https://docs.sats.id/sats-names/protocol-spec#validate-names-1
    let result = parse_sats_name_body(dotsats.name);
    match result {
        Some(s) => return Ok(s),
        None => return Err(()),
    }
}

fn parse_sats_name_body(input: &str) -> Option<String> {
    let input = input.to_lowercase();
    let mut parts = input.splitn(2, |c| c == ' ' || c == '\n');
    let first_part = parts.next()?.trim();

    let period_count = first_part.chars().filter(|&c| c == '.').count();
    if period_count != 1 {
        return None;
    }

    if !first_part.ends_with(".sats") {
        return None;
    }

    Some(first_part.to_string())
}
