extern crate rusty_leveldb;
use bitcoin::{blockdata::block::Header, consensus::Decodable, hashes::Hash};
use rusty_leveldb::{LdbIterator, Options, DB};

use std::{
    io::Cursor,
    io::{BufReader, Read, Seek, SeekFrom},
    path::PathBuf,
};

use crate::{read_varint_core, BlockUndo};

// Define the structs
#[derive(Debug)]
#[allow(dead_code)]
pub struct BlockIndexRecord {
    pub height: u32,
    pub num_transactions: u32,
    validation_status: BlockStatus,
    file: Option<u32>,
    block_offset: Option<u64>,
    undo_offset: Option<u64>,
    pub header: Header,
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
    block_obfs: Option<Vec<u8>>,
    chain_state: rusty_leveldb::DB,
    chain_obfs: Option<Vec<u8>>,
    datadir: PathBuf,
    #[allow(dead_code)]
    last_file_number: u32,
    genesis: bitcoin::Block,
    pub genesis_hash: bitcoin::BlockHash,
    pub tip_hash: bitcoin::BlockHash,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
    pub struct BlockStatus: u32 {
        //const BLOCK_VALID_UNKNOWN      = 0;
        const BLOCK_VALID_HEADER       = 1;
        const BLOCK_VALID_TREE         = 2;
        const BLOCK_VALID_TRANSACTIONS = 3;
        const BLOCK_VALID_CHAIN        = 4;
        const BLOCK_VALID_SCRIPTS      = 5;
        const BLOCK_VALID_MASK         = Self::BLOCK_VALID_HEADER.bits()
                                        | Self::BLOCK_VALID_TREE.bits()
                                        | Self::BLOCK_VALID_TRANSACTIONS.bits()
                                        | Self::BLOCK_VALID_CHAIN.bits()
                                        | Self::BLOCK_VALID_SCRIPTS.bits();
        const BLOCK_HAVE_DATA          = 8;
        const BLOCK_HAVE_UNDO          = 16;
        const BLOCK_HAVE_MASK          = Self::BLOCK_HAVE_DATA.bits() | Self::BLOCK_HAVE_UNDO.bits();
        const BLOCK_FAILED_VALID       = 32;
        const BLOCK_FAILED_CHILD       = 64;
        const BLOCK_FAILED_MASK        = Self::BLOCK_FAILED_VALID.bits() | Self::BLOCK_FAILED_CHILD.bits();
    }
}

impl Scanner {
    pub fn new(datadir: PathBuf) -> Self {
        let mut block_index = datadir.clone();
        let mut chain_state = datadir.clone();
        block_index.push("blocks");
        block_index.push("index");
        chain_state.push("chainstate");

        let options = Options {
            create_if_missing: false,
            ..Default::default()
        };

        let mut block_db = match DB::open(&block_index, options.clone()) {
            Ok(db) => db,
            Err(e) => match e.code {
                rusty_leveldb::StatusCode::LockError => panic!("Please close bitcoin core first"),
                _ => panic!("Error opening database: {:?}", e),
            },
        };

        let obfuscate_key = b"\x0e\x00obfuscate_key";

        let block_obfs = block_db.get(obfuscate_key).map(|value| value[1..].to_vec());

        // let obfuscate_value = block_db
        //     .get(obfuscate_key)
        //     .expect("Failed to read obfuscation key");

        // dbg!("If this hangs, please run bitcoind --reindex-chainstate");
        let mut chain_db = match DB::open(&chain_state, options) {
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
        let chain_obfs = chain_db.get(obfuscate_key).map(|value| value[1..].to_vec());

        let tip = chain_db.get(b"B").unwrap();
        let tip = Self::obfs(&chain_obfs, &tip);
        let tip_hash: bitcoin::BlockHash = Hash::from_slice(&tip).unwrap();

        // get the last file number
        let key = b"l";
        let value = block_db.get(key).unwrap();
        let value = u32::from_le_bytes(value[..4].try_into().unwrap());
        let last_file_number = value;

        let genesis = Self::read_genesis(datadir.clone());
        let genesis_hash = genesis.block_hash();

        Self {
            datadir,
            block_index: block_db,
            chain_state: chain_db,
            last_file_number,
            genesis,
            genesis_hash,
            tip_hash,
            chain_obfs,
            block_obfs,
        }
    }

    pub fn scan_blocks_db<F>(&mut self, mut f: F)
    where
        F: FnMut(Vec<u8>, Vec<u8>),
    {
        let mut it = self.block_index.new_iter().unwrap();

        while let Some((key, value)) = it.next() {
            let value = Self::obfs(&self.block_obfs, &value);
            f(key, value);
        }
    }

    pub fn scan_chain_db<F>(&mut self, mut f: F)
    where
        F: FnMut(Vec<u8>, Vec<u8>),
    {
        let mut it = self.chain_state.new_iter().unwrap();

        while let Some((key, value)) = it.next() {
            let value = Self::obfs(&self.chain_obfs, &value);
            f(key, value);
        }
    }

    #[allow(dead_code)]
    fn chain_get(&mut self, key: &[u8]) -> Vec<u8> {
        let value = self.chain_state.get(key).unwrap();
        Self::obfs(&self.chain_obfs, &value)
    }

    fn obfs(obfs: &Option<Vec<u8>>, value: &[u8]) -> Vec<u8> {
        match obfs {
            Some(obfs) => value
                .iter()
                .enumerate()
                .map(|(i, &byte)| byte ^ obfs[i % obfs.len()])
                .collect(),
            None => value.to_owned(),
        }
    }

    pub fn block_index_record(&mut self, key: &bitcoin::BlockHash) -> BlockIndexRecord {
        let key1 = b"b";
        let key2 = &key.as_raw_hash().to_byte_array()[..];
        let key = [key1, key2].concat();
        let value = self.block_index.get(&key).unwrap();

        let mut r = Cursor::new(value);

        let record_version = read_varint_core(&mut r).unwrap() as u32;
        // test the code base with earlier versions and see if it works
        // please PR if it does
        more_asserts::assert_ge!(record_version, 220000);
        let height = read_varint_core(&mut r).unwrap() as u32;
        let validation_status = read_varint_core(&mut r).unwrap() as u32;
        let validation_status = BlockStatus::from_bits_truncate(validation_status);
        let num_transactions = read_varint_core(&mut r).unwrap() as u32;

        let file = if validation_status.contains(BlockStatus::BLOCK_HAVE_DATA)
            || validation_status.contains(BlockStatus::BLOCK_HAVE_UNDO)
        {
            //dbg!(validation_status);
            Some(read_varint_core(&mut r).unwrap() as u32)
        } else {
            None
        };

        let block_offset = if validation_status.contains(BlockStatus::BLOCK_HAVE_DATA) {
            Some(read_varint_core(&mut r).unwrap())
        } else {
            None
        };

        let undo_offset = if validation_status.contains(BlockStatus::BLOCK_HAVE_UNDO) {
            Some(read_varint_core(&mut r).unwrap())
        } else {
            None
        };

        let header = Header::consensus_decode(&mut r).unwrap();

        // we've read all the data
        assert!(r.position() == r.get_ref().len() as u64);

        BlockIndexRecord {
            header,
            height,
            num_transactions,
            validation_status,
            file,
            block_offset,
            undo_offset,
        }
    }

    fn read_genesis(datadir: PathBuf) -> bitcoin::Block {
        let file = datadir.join("blocks").join("blk00000.dat");
        let mut file = std::fs::File::open(file)
            .expect("First blk file not found, do you have pruning enabled?");
        let mut magic_size = [0; 8];
        // todo check magic
        file.read_exact(&mut magic_size).unwrap();
        let size = magic_size[4..8].try_into().unwrap();
        let _size = u32::from_le_bytes(size);

        bitcoin::Block::consensus_decode(&mut file).unwrap()
    }

    pub fn read_block(&mut self, id: &bitcoin::BlockHash) -> bitcoin::Block {
        let record = self.block_index_record(id);
        self.read_block_from_record(&record)
    }

    pub fn read_block_from_record(&mut self, record: &BlockIndexRecord) -> bitcoin::Block {
        let file = self
            .datadir
            .clone()
            .join("blocks")
            .join(format!("blk{:05}.dat", record.file.unwrap()));

        let mut file = std::fs::File::open(file).unwrap();
        let mut magic_size = [0; 8];
        // todo check magic

        file.seek(SeekFrom::Start(record.block_offset.unwrap() - 8))
            .unwrap();

        file.read_exact(&mut magic_size).unwrap();
        let size = magic_size[4..8].try_into().unwrap();
        let _size = u32::from_le_bytes(size);

        bitcoin::Block::consensus_decode(&mut file).unwrap()
    }

    pub fn read_undo(&mut self, id: &bitcoin::BlockHash) -> BlockUndo {
        let block_index_record = self.block_index_record(id);

        // dbg!(&block_index_record);

        let file = self
            .datadir
            .clone()
            .join("blocks")
            .join(format!("rev{:05}.dat", block_index_record.file.unwrap()));

        let mut file = std::fs::File::open(file).unwrap();

        // dbg!(block_index_record.undo_offset);

        file.seek(SeekFrom::Start(block_index_record.undo_offset.unwrap() - 8))
            .unwrap();
        let mut magic_size = [0; 8];
        file.read_exact(&mut magic_size).unwrap();
        let size = magic_size[4..8].try_into().unwrap();
        let _size = u32::from_le_bytes(size);

        BlockUndo::parse(&mut file, Some(block_index_record.num_transactions)).unwrap()
        //let undo = (&mut file).unwrap();
    }

    pub fn genesis(&self) -> bitcoin::Block {
        self.genesis.clone()
    }

    pub fn genesis_hash(&self) -> bitcoin::BlockHash {
        self.genesis_hash
    }
}
