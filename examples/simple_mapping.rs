#[path = "util.rs"]
mod util;

extern crate serde;
extern crate serde_derive;
extern crate serde_json;

extern crate ord_labs;
use ord_labs::*;

use async_std::task::block_on;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::opcodes::all::*;
use bitcoin::{BlockHash, Transaction};
use bitcoin_scanner::db::{InscriptionRecord, SatsNameRecord, DB};
use bitcoin_scanner::{Scanner, TxInUndo};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug)]
struct DotSats<'a> {
    p: &'a str,
    op: &'a str,
    name: &'a str,
}

pub fn main() {
    let data_dir = util::bitcoin_data_dir(bitcoin::Network::Bitcoin);

    let mut scanner = Scanner::new(data_dir);
    let tip_hash = scanner.tip_hash;
    let _tip = scanner.read_block(&tip_hash);

    // Flip here to write to std::out in TSV format instead.
    let use_db = true;
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
        let mut sats_name_count = 0;

        // Only for debugging
        // println!("Block len {0:?}", block.txdata.len());
        // println!("Undo len {0:?}", undo.inner.len());

        block.txdata.iter().enumerate().for_each(|(tx_i, tx)| {
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

                let script = bitcoin::ScriptBuf::from(tx.input[i].witness.tapscript().unwrap());
                for instruction in script.instructions() {
                    match instruction {
                        Ok(bitcoin::blockdata::script::Instruction::PushBytes(data)) => {
                            if data.len() == 32 {
                                let mut x_only_pubkey = [0u8; 32];
                                x_only_pubkey.copy_from_slice(data.as_bytes());
                                possible_inscriber = x_only_pubkey;
                            }
                        }
                        Ok(bitcoin::blockdata::script::Instruction::Op(op)) => match op.to_u8() {
                            x if x == OP_CHECKSIG.to_u8()
                                || x == OP_CHECKSIGVERIFY.to_u8()
                                || x == OP_CHECKSIGADD.to_u8() =>
                            {
                                if possible_inscriber != [0; 32] {
                                    inscribers.push(possible_inscriber);
                                    possible_inscriber = [0; 32];
                                }
                            }
                            _ => {}
                        },
                        Err(err) => {
                            println!("Error parsing instruction: {}", err);
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
                let hash_result;
                let digest = match ins.body() {
                    Some(body) => {
                        hash_result = sha256::Hash::hash(body);
                        hash_result.as_byte_array()
                    }
                    None => &[0u8; 32],
                };

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
                    digest: *digest,
                };

                ins_block_count += 1;

                let maybe_sats_name = identify_sats_name(ins);

                // TODO: Not really async for now for ease of debugging.
                // TBD: Async strategy
                if use_db {
                    block_on(async {
                        let ins_res = db.insert_inscription(&insert_rec).await;

                        if maybe_sats_name.is_ok() {
                            sats_name_count += 1;
                            let sats_name_rec = SatsNameRecord {
                                _id: 0,
                                inscription_record_id: ins_res.unwrap(),
                                short_input_id: short_input_id,
                                name: maybe_sats_name.unwrap(),
                            };

                            let _res = db.insert_sats_name(&sats_name_rec).await;
                        }
                    });
                } else {
                    let _res = print_tsv(&insert_rec);
                }
            }

            blk_tx_count += 1;
        });

        println!(
            "Processed block {:?}, inserted {} inscription records and {} sats names.",
            record.height, ins_block_count, sats_name_count
        );

        // We can stop scanning once we have parsed the block containing inscription #0, i.e. block
        // height 767430.
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

fn print_tsv(r: &InscriptionRecord) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    write!(
        &mut handle,
        "{:?}\t{:?}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}",
        r.commit_output_script,
        r.txid,
        r.index,
        r.genesis_inscribers,
        r.genesis_amount,
        r.address,
        r.content_length,
        r.content_type,
        r.genesis_block_hash,
        r.genesis_fee,
        r.genesis_height,
        r.short_input_id
    )?;
    writeln!(&mut handle)?;

    Ok(())
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
