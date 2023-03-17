extern crate rusty_leveldb;
use bitcoin::{consensus::Decodable, hashes::Hash};
use hex;
use rusty_leveldb::{LdbIterator, Options, DB};

use std::{
    io::Cursor,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

use crate::read_varint_core;

// Define the structs
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct BlockIndexRecord {
    header: Vec<u8>,
    height: u32,
    num_transactions: u32,
    validation_status: u8,
    block_data_file: u32,
    block_data_offset: u64,
    undo_data_file: u32,
    undo_data_offset: u64,
}

#[derive(Debug)]
#[allow(dead_code)]
struct FileInformationRecord {
    num_blocks: u32,
    block_file_size: u64,
    undo_file_size: u64,
    lowest_height: u32,
    highest_height: u32,
    lowest_timestamp: u32,
    highest_timestamp: u32,
}

#[derive(Debug)]
#[allow(dead_code)]
struct TransactionIndexRecord {
    block_file_number: u32,
    block_offset: u64,
    tx_offset: u64,
}

pub struct Scanner {
    block_index: rusty_leveldb::DB,
    chain_state: rusty_leveldb::DB,
    chain_obfs: Vec<u8>,
    datadir: PathBuf,
    #[allow(dead_code)]
    last_file_number: u32,
    genesis: bitcoin::Block,
    pub genesis_hash: bitcoin::BlockHash,
    pub tip_hash: bitcoin::BlockHash,
}

impl Scanner {
    pub fn new(datadir: PathBuf) -> Self {
        let mut block_index = datadir.clone();
        let mut chain_state = datadir.clone();
        block_index.push("blocks");
        block_index.push("index");
        chain_state.push("chainstate");

        let mut options = Options::default();
        options.create_if_missing = false;

        let mut block_db = match DB::open(&block_index, options.clone()) {
            Ok(db) => db,
            Err(e) => match e.code {
                rusty_leveldb::StatusCode::LockError => panic!("Please close bitcoin core first"),
                _ => panic!("Error opening database: {:?}", e),
            },
        };

        // dbg!("If this hangs, please run bitcoind --reindex-chainstate");
        let mut chain_db = match DB::open(&chain_state, options.clone()) {
            Ok(db) => db,
            Err(e) => {
                let code = e.code;
                match code {
                    rusty_leveldb::StatusCode::LockError => {
                        panic!("Please close bitcoin core first")
                    }
                    _ => panic!(
                        "Error opening chain state database: code {:?}, message {:?}",
                        code, e.err
                    ),
                }
            }
        };

        let obfuscate_key = b"\x0e\x00obfuscate_key";
        let obfuscate_value = chain_db
            .get(obfuscate_key)
            .expect("Failed to read obfuscation key");
        let chain_obfs = obfuscate_value[1..].to_vec();

        let obfuscated_value = chain_db.get(b"B").unwrap();

        let tip = Self::obfs(&chain_obfs, &obfuscated_value);

        // get the last file number
        let key = b"l";
        let value = block_db.get(key).unwrap();
        let value = u32::from_le_bytes(value[..4].try_into().unwrap());
        let last_file_number = value;

        let genesis = Self::read_genesis(datadir.clone());
        let genesis_hash = genesis.block_hash();

        dbg!(hex::encode(&tip));

        let tip_hash: bitcoin::BlockHash = bitcoin::hashes::Hash::from_slice(&tip).unwrap();
        //let tip_hash = bitcoin::hashes::sha256d::Hash::from_slice(&tip);
        //let tip_hash = BlockHash::from_hash(tip_hash.unwrap());

        Self {
            datadir: datadir,
            block_index: block_db,
            chain_state: chain_db,
            last_file_number,
            genesis,
            genesis_hash,
            tip_hash,
            chain_obfs,
        }
    }

    pub fn scan_block_index<F>(mut self, f: F)
    where
        F: Fn(Vec<u8>, Vec<u8>),
    {
        let mut it = self.block_index.new_iter().unwrap();

        while let Some((key, value)) = it.next() {
            f(key, value);
        }
    }

    #[allow(dead_code)]
    fn chain_get(&mut self, key: &[u8]) -> Vec<u8> {
        let obfs = &self.chain_obfs;
        let value = self.chain_state.get(key).unwrap();
        Self::obfs(obfs, &value)
    }

    fn obfs(obfs: &[u8], value: &[u8]) -> Vec<u8> {
        value
            .iter()
            .enumerate()
            .map(|(i, &byte)| byte ^ obfs[i % obfs.len()])
            .collect()
    }

    pub fn block_index_record(&mut self, key: bitcoin::BlockHash) -> BlockIndexRecord {
        let key1 = b"b";

        let key2 = key.as_inner().as_slice();

        let key = [key1, key2].concat();

        let value = self.block_index.get(&key).unwrap();

        let mut r = Cursor::new(value);

        let record_version = read_varint_core(&mut r).unwrap() as u32;
        assert!(record_version >= 240001);
        let height = read_varint_core(&mut r).unwrap() as u32;
        let validation_status = read_varint_core(&mut r).unwrap() as u8;
        let num_transactions = read_varint_core(&mut r).unwrap() as u32;
        let block_data_file = read_varint_core(&mut r).unwrap() as u32;
        let block_data_offset = read_varint_core(&mut r).unwrap();
        let undo_data_offset = read_varint_core(&mut r).unwrap();
        let undo_data_file = read_varint_core(&mut r).unwrap() as u32;
        let mut buf = [0; 4];
        r.read_exact(&mut buf).unwrap();
        let header = buf.to_vec();

        BlockIndexRecord {
            header,
            height,
            num_transactions,
            validation_status,
            block_data_file,
            block_data_offset,
            undo_data_file,
            undo_data_offset,
            ..Default::default()
        }
    }

    fn read_genesis(datadir: PathBuf) -> bitcoin::Block {
        let file = datadir.clone().join("blocks").join("blk00000.dat");
        let mut file = std::fs::File::open(file).unwrap();
        let mut magic_size = [0; 8];
        // todo check magic
        file.read_exact(&mut magic_size).unwrap();
        let size = magic_size[4..8].try_into().unwrap();
        let _size = u32::from_le_bytes(size);

        let block = bitcoin::Block::consensus_decode(&mut file).unwrap();
        block
    }

    pub fn read_block(&mut self, id: bitcoin::BlockHash) -> bitcoin::Block {
        let block_index_record = self.block_index_record(id);

        let file = self
            .datadir
            .clone()
            .join("blocks")
            .join(format!("blk{:05}.dat", block_index_record.block_data_file));

        let mut file = std::fs::File::open(file).unwrap();
        let mut magic_size = [0; 8];
        // todo check magic

        file.seek(SeekFrom::Start(block_index_record.block_data_offset - 8))
            .unwrap();

        file.read_exact(&mut magic_size).unwrap();
        let size = magic_size[4..8].try_into().unwrap();
        let _size = u32::from_le_bytes(size);

        let block = bitcoin::Block::consensus_decode(&mut file).unwrap();
        block
    }

    pub fn genesis(&self) -> bitcoin::Block {
        self.genesis.clone()
    }

    pub fn genesis_hash(&self) -> bitcoin::BlockHash {
        self.genesis_hash
    }
}
