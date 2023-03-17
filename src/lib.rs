use bitcoin::Script;
use byteorder::{LittleEndian, ReadBytesExt};
use std::fs::File;
use std::io::{self, Error, ErrorKind, Read, Result};

const _MAX_SIZE: u64 = 0x02000000;
mod scanner;
pub use scanner::Scanner;

#[allow(dead_code)]
fn parse_rev_file(file: &mut File) -> io::Result<()> {
    const MAGIC_BYTES: [u8; 4] = [0xf9, 0xbe, 0xb4, 0xd9]; // Mainnet magic bytes
    const _HEADER_SIZE: u64 = 8; // 4 bytes for magic number + 4 bytes for data size

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
        let _data_size = u32::from_le_bytes(data_size_bytes) as u64;

        dbg!("block", _data_size);

        let block_undo = BlockUndo::parse(file, None)?;
        dbg!(block_undo.inner.len());

        let mut undo_dsha = vec![0; 32];
        file.read_exact(&mut undo_dsha)?;
    }

    Ok(())
}

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
    pub dsha: [u8; 32],
}

impl BlockUndo {
    pub fn parse<R: Read>(reader: &mut R, ntxs: Option<u32>) -> io::Result<Self> {
        let ntxs = match ntxs {
            Some(ntxs) => {
                let file_ntxs = read_compact_size(reader)? as u32 + 1;
                assert_eq!(file_ntxs, ntxs);
                ntxs
            }
            None => read_compact_size(reader)? as u32 + 1,
        };

        let mut inner = vec![TxUndo::default(); ntxs as usize];

        for tx_undo in inner.iter_mut().skip(1) {
            let ntxi = read_compact_size(reader)?;
            for _ in 0..ntxi {
                tx_undo.0.push(TxInUndo::parse(reader)?);
            }
        }

        let mut dsha = [0; 32];
        reader.read_exact(&mut dsha)?;

        Ok(Self { inner, dsha })
    }
}

impl TxInUndo {
    fn parse<R: Read>(reader: &mut R) -> Result<Self> {
        let code = read_varint_core(reader)?;
        let coinbase = code & 1;
        let height = code >> 1;

        let version = read_varint_core(reader)?;
        assert_eq!(version, 0);
        let amount = decompress_amount(read_varint_core(reader)?);
        let kind = read_varint_core(reader)?;

        let script = match kind {
            0 => {
                // p2pkh
                let mut script = vec![0x76, 0xa9, 20];
                let mut buf = vec![0; 20];
                reader.read_exact(&mut buf)?;
                script.extend_from_slice(&buf);
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
                // TODO: decompress pubkey
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

        let script: Script = Script::from(script);
        let asm = script.clone().asm();
        dbg!(asm);

        Ok(Self {
            coinbase,
            height,
            script,
            amount,
        })
    }
}

fn read_compact_size<R: Read>(r: &mut R) -> Result<u64> {
    let n = r.read_u8()?;
    match n {
        0xFF => {
            let x = r.read_u64::<LittleEndian>()?;
            if x < 0x100000000 {
                Err(Error::new(ErrorKind::Other, "oh no!"))
            } else {
                Ok(x)
            }
        }
        0xFE => {
            let x = r.read_u32::<LittleEndian>()?;
            if x < 0x10000 {
                Err(Error::new(ErrorKind::Other, "oh no!"))
            } else {
                Ok(x as u64)
            }
        }
        0xFD => {
            let x = r.read_u16::<LittleEndian>()?;
            if x < 0xFD {
                Err(Error::new(ErrorKind::Other, "oh no!"))
            } else {
                Ok(x as u64)
            }
        }
        n => Ok(n as u64),
    }
}

fn read_varint_core<R: Read>(r: &mut R) -> Result<u64> {
    let mut n: u64 = 0;
    loop {
        let mut ch_data = [0; 1];
        r.read_exact(&mut ch_data)?;
        let ch_data = ch_data[0];
        n = (n << 7) | (ch_data & 0x7F) as u64;
        if ch_data & 0x80 != 0 {
            n += 1;
        } else {
            return Ok(n);
        }
    }
}

// Amount compression:
// * If the amount is 0, output 0
// * first, divide the amount (in base units) by the largest power of 10 possible; call the exponent e (e is max 9)
// * if e<9, the last digit of the resulting number cannot be 0; store it as d, and drop it (divide by 10)
//   * call the result n
//   * output 1 + 10*(9*n + d - 1) + e
// * if e==9, we only know the resulting number is not zero, so output 1 + 10*(n - 1) + 9
// (this is decodable, as d is in [1-9] and e is in [0-9])
#[allow(dead_code)]
fn compress_amount(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut e = 0;
    let mut n = n;
    while n % 10 == 0 && e < 9 {
        n /= 10;
        e += 1;
    }
    if e < 9 {
        let d = (n % 10) as usize;
        assert!(d >= 1 && d <= 9);
        n /= 10;
        1 + (n * 9 + d as u64 - 1) * 10 + e as u64
    } else {
        1 + (n - 1) * 10 + 9
    }
}

fn decompress_amount(x: u64) -> u64 {
    // x = 0  OR  x = 1+10*(9*n + d - 1) + e  OR  x = 1+10*(n - 1) + 9
    if x == 0 {
        return 0;
    }
    let mut x = x - 1;
    // x = 10*(9*n + d - 1) + e
    let e = (x % 10) as usize;
    x /= 10;
    let n;
    if e < 9 {
        // x = 9*n + d - 1
        let d = (x % 9) + 1;
        x /= 9;
        // x = n
        n = x * 10 + d;
    } else {
        n = x + 1;
    }
    (0..e).fold(n, |acc, _| acc * 10)
}
