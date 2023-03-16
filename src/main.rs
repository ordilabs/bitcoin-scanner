use byteorder::{LittleEndian, ReadBytesExt};
use std::env;
use std::fs::File;
use std::io::{self, Error, ErrorKind, Read, Result};
//use undo::{UndoBlock, UndoCoin};
const MAX_SIZE: u64 = 0x02000000;

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} /path/to/rev*.dat", args[0]);
        std::process::exit(1);
    }

    let filepath = &args[1];
    let mut file = File::open(filepath)?;

    parse_rev_file(&mut file)?;

    Ok(())
}

fn parse_rev_file(file: &mut File) -> io::Result<()> {
    const MAGIC_BYTES: [u8; 4] = [0xf9, 0xbe, 0xb4, 0xd9]; // Mainnet magic bytes
    const HEADER_SIZE: u64 = 8; // 4 bytes for magic number + 4 bytes for data size

    loop {
        let mut magic = [0; 4];
        let read_magic = file.read(&mut magic)?;
        if read_magic == 0 {
            break; // EOF reached
        }

        if magic != MAGIC_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid magic bytes",
            ));
        }

        let mut data_size_bytes = [0; 4];

        file.read_exact(&mut data_size_bytes)?;
        let data_size = u32::from_le_bytes(data_size_bytes) as u64;

        //let undo = UndoBlock::parse(file)?;

        let block_undo = BlockUndo::parse(file)?;
        dbg!(block_undo.inner.len());

        let mut undo_dsha = vec![0; 32];
        file.read_exact(&mut undo_dsha)?;
    }

    Ok(())
}

//use byteorder::{LittleEndian, ReadBytesExt};

use bitcoin::Script;
use bitcoin::{
    blockdata,
    consensus::{self},
};

#[derive(Debug, Clone)]
pub struct TxInUndo {
    pub coinbase: u64,
    pub height: u64,
    pub script: Script,
    pub amount: u64,
}

#[derive(Debug, Default, Clone)]
pub struct TxUndo(Vec<TxInUndo>);

pub struct BlockUndo {
    pub inner: Vec<TxUndo>,
    pub dsha: Vec<u8>,
}

impl BlockUndo {
    pub fn parse<R: Read>(reader: &mut R) -> io::Result<Self> {
        let ntxs = read_compact_size(&mut reader)?;
        let mut inner = vec![TxUndo::default(); (ntxs + 1) as usize];

        for tx_undo in inner.iter_mut().skip(1) {
            let ntxi = read_compact_size(&mut reader)?;
            for _ in 0..ntxi {
                tx_undo.0.push(TxInUndo::parse(&mut reader)?);
            }
        }

        let mut dsha = vec![0; 32];
        reader.read_exact(&mut dsha)?;

        Ok(Self { inner, dsha })
    }
}

impl TxInUndo {
    fn parse<R: Read>(reader: &mut R) -> Result<Self> {
        let code = read_varint(&mut reader)?;
        let coinbase = code & 1;
        let height = code >> 1;

        let version = read_varint(&mut reader)?;
        assert_eq!(version, 0);
        let amount = decompress_amount(read_varint(&mut reader)?);
        let kind = read_varint(&mut reader)?;

        let script = match kind {
            0 => {
                // p2pkh
                let mut script = vec![0x76, 0xa9, 20];
                script.extend_from_slice(&[0x88, 0xac]);
                script
            }
            1 => {
                // p2sh
                let mut script = vec![0xa9, 20];
                let mut buf = vec![0; 20];
                reader.read_exact(&mut buf)?;
                script.extend_from_slice(&buf);
                script.push(0x87);
                script
            }
            2..=5 => {
                // p2pk, fake implementaion! decompressing pubkey not implemented
                let sz = 32;
                let mut script = vec![0; sz];
                reader.read_exact(&mut script)?;
                script
            }
            _ => {
                let sz = (kind - 6) as usize;
                let mut script = vec![0; sz];
                reader.read_exact(&mut script)?;
                script
            }
        };

        let script: blockdata::script::Script = consensus::deserialize(&script).unwrap();

        Ok(Self {
            coinbase,
            height,
            script,
            amount,
        })
    }
}

fn read_compact_size<R: Read>(reader: &mut R) -> Result<u64> {
    let ch_size = reader.read_u8()?;

    let n_size_ret = match ch_size {
        0..=252 => ch_size as u64,
        253 => {
            let size = reader.read_u16::<LittleEndian>()? as u64;
            if size < 253 {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "non-canonical ReadCompactSize()",
                ));
            }
            size
        }
        254 => {
            let size = reader.read_u32::<LittleEndian>()? as u64;
            if size < 0x10000 {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "non-canonical ReadCompactSize()",
                ));
            }
            size
        }
        255 => {
            let size = reader.read_u64::<LittleEndian>()?;
            if size < 0x100000000 {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "non-canonical ReadCompactSize()",
                ));
            }
            size
        }
    };

    Ok(n_size_ret)
}

fn read_varint<R: Read>(reader: &mut R) -> Result<u64> {
    let first_byte = reader.read_u8()?;

    let result = match first_byte {
        0xFD => reader.read_u16::<byteorder::LittleEndian>()? as u64,
        0xFE => reader.read_u32::<byteorder::LittleEndian>()? as u64,
        0xFF => reader.read_u64::<byteorder::LittleEndian>()?,
        _ => first_byte as u64,
    };

    Ok(result)
}

fn decompress_amount(x: u64) -> u64 {
    if x == 0 {
        return 0;
    }
    let x = x - 1;
    // x = 10*(9*n + d - 1) + e
    let (x, e) = (x / 10, x % 10);
    let mut n = 0;
    if e < 9 {
        // x = 9*n + d - 1
        let (x, d) = (x / 9, x % 9);
        let d = d + 1;
        // x = n
        n = x * 10 + d;
    } else {
        n = x + 1;
    }
    n * 10u64.pow(e as u32)
}
