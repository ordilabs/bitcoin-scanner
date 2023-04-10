#[path = "util.rs"]
mod util;

extern crate serde;
extern crate serde_derive;
extern crate serde_json;

extern crate ord_labs;
use ord_labs::*;

use async_std::task::block_on;
use bitcoin::opcodes::all::*;
use bitcoin::{BlockHash, Transaction};
use bitcoin_scanner::db::{InscriptionRecord, DB};
use bitcoin_scanner::{Scanner, TxInUndo};
use std::str::FromStr;

pub fn main() {
    let data_dir = util::bitcoin_data_dir(bitcoin::Network::Bitcoin);

    let mut scanner = Scanner::new(data_dir);
    let tip_hash = scanner.tip_hash;
    let _tip = scanner.read_block(&tip_hash);

    let mut db = DB::setup(true).unwrap();

    let mut current_hash = tip_hash;

    loop {
        let record = scanner.block_index_record(&current_hash);
        let block = scanner.read_block_from_record(&record);
        let undo = scanner.read_undo(&current_hash);

        // Keeping the current input count on the block we are parsing in order
        // to look up the corresponding spent UTXO from the rev file to get the
        // pubkey that was spent from.
        let mut blk_tx_count = 0;

        // Count inscriptions found in each block.
        let mut ins_block_count = 0;

        // Only for debugging
        // println!("Block len {0:?}", block.txdata.len());
        // println!("Undo len {0:?}", undo.inner.len());

        block.txdata.iter().for_each(|tx| {
            // Weirdly not needed it seems
            // if tx.is_coin_base() {
            //     return;
            // };

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

                let insert_rec = InscriptionRecord {
                    _id: 0,
                    commit_output_script: spk,
                    txid: txid,
                    index: i,
                    genesis_inscribers: inscribers,
                    genesis_amount: tx.output[i].value,
                    address: address.to_string(),
                    content_length: ins.body().unwrap().len(),
                    content_type: ins.content_type().unwrap().to_string(),
                    genesis_block_hash: block_hash,
                    genesis_fee: calculate_fee(tx, tx_ins),
                    genesis_height: record.height,
                };

                ins_block_count += 1;

                // TODO: Not really async for now for ease of debugging.
                // TBD: Async strategy
                block_on(async {
                    let _res = db.insert(&insert_rec).await;
                });
            }

            blk_tx_count += 1;
        });

        println!(
            "Processed block {:?}, inserted {} inscription records.",
            record.height, ins_block_count
        );

        // We can stop scanning once we have parsed the block containing inscription #0
        let stop_block =
            BlockHash::from_str("000000000000000000029730547464F056F8B6E2E0A02EAF69C24389983A04F5")
                .unwrap();
        if current_hash == stop_block {
            println!("Finished scanning!");
            break;
        }

        current_hash = record.header.prev_blockhash;
    }
}

fn calculate_fee(tx: &Transaction, tx_undo: &Vec<TxInUndo>) -> u64 {
    let input_sum: u64 = tx_undo.iter().map(|input| input.amount).sum();

    let output_sum: u64 = tx.output.iter().map(|output| output.value).sum();

    input_sum - output_sum
}
