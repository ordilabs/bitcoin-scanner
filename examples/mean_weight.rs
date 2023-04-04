// use std::collections::HashMap;

use std::collections::HashMap;

use bitcoin_scanner::Scanner;

#[path = "util.rs"]
mod util;

//mod test;

//use test::bitcoin_data_dir;

fn main() {
    let data_dir = util::bitcoin_data_dir(bitcoin::Network::Regtest);

    println!("Using bitcoin data_dir: {data_dir:?}");
    let mut scanner = Scanner::new(&data_dir);

    println!("{}", scanner.tip_hash);

    let genesis_hash = scanner.genesis_hash();
    println!("Genesis hash: {genesis_hash}");

    let genesis_record = scanner.block_index_record(&genesis_hash);
    dbg!(genesis_record);

    let tip_hash = scanner.tip_hash;
    dbg!(tip_hash);
    let tip_record = scanner.block_index_record(&tip_hash);
    dbg!(tip_record);

    let tip = scanner.read_block(&tip_hash);
    dbg!(tip.txdata.len());

    let tip_undo = scanner.read_undo(&tip_hash);
    dbg!(tip_undo.inner.len());

    // let h1_hash =
    //     BlockHash::from_str("0ecc17a4f15794630e86a11957c037cee269c3117b0a4a67513a58124261f968")
    //         .unwrap();

    // let h1_record = scanner.block_index_record(&h1_hash);
    // dbg!(h1_record);
    // let h1 = scanner.read_block(&h1_hash);
    // let h1_undo = scanner.read_undo(&h1_hash);
    // dbg!(h1_undo.inner.len());
    // dbg!(h1.txdata.len());

    // //    dbg!(scanner.genesis_hash());
    let mut counts: HashMap<u8, usize> = HashMap::new();
    scanner.scan_blocks_db(|key, _value| {
        let count = counts.entry(key[0]).or_insert(0);
        *count += 1;
        // {
        //     let key = hex::encode(key);
        //     let value = hex::encode(value);
        //     println!("key: {}, value: {}", key, value);
        // }
    });

    // let undo = scanner.read_undo(&tip_hash);
    // dbg!(undo.inner.len());

    // println!("Counts of the first byte in keys:");
    // for (byte, count) in &counts {
    //     println!("{}: {}", byte.to_owned() as char, count);
    // }
}
