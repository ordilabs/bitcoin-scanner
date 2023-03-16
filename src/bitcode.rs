use byteorder::{LittleEndian, ReadBytesExt};
use std::io::Read;

pub struct UndoCoin {
    pub coinbase: u64,
    pub height: u64,
    pub script: Vec<u8>,
    pub amount: u64,
}

pub struct UndoBlock {
    pub undotxs: Vec<Vec<UndoCoin>>,
}

impl UndoBlock {
    pub fn parse<R: Read>(mut reader: R) -> Result<Self, std::io::Error> {
        let ntxs = parse_compact(&mut reader)?;
        let mut undotxs = vec![vec![]; ntxs + 1];

        for undotx in undotxs.iter_mut().skip(1) {
            let ntxi = parse_compact(&mut reader)?;
            for _ in 0..ntxi {
                undotx.push(UndoCoin::parse(&mut reader)?);
            }
        }

        Ok(Self { undotxs })
    }
}

impl UndoCoin {
    fn parse<R: Read>(mut reader: R) -> Result<Self, std::io::Error> {
        let code = parse_varint(&mut reader)?;
        let coinbase = code & 1;
        let height = code >> 1;

        let version = parse_varint(&mut reader)?;
        assert_eq!(version, 0);
        let amount = decompress_amount(parse_varint(&mut reader)?);
        let kind = parse_varint(&mut reader)?;

        let script = match kind {
            0 => {
                let mut script = vec![0x76, 0xa9, 20];
                script.extend(reader.by_ref().take(20));
                script.extend_from_slice(&[0x88, 0xac]);
                script
            }
            1 => {
                let mut script = vec![0xa9, 20];
                script.extend(reader.by_ref().take(20));
                script.push(0x87);
                script
            }
            2..=5 => {
                let sz = 32;
                let mut script = vec![0; sz];
                reader.read_exact(&mut script)?;
                script
            }
            _ => {
                let sz = kind - 6;
                let mut script = vec![0; sz];
                reader.read_exact(&mut script)?;
                script
            }
        };

        Ok(Self {
            coinbase,
            height,
            script,
            amount,
        })
    }
}

fn parse_compact<R: Read>(reader: &mut R) -> Result<usize, std::io::Error> {
    reader.read_u32::<LittleEndian>().map(|x| x as usize)
}

fn parse_varint<R: Read>(reader: &mut R) -> Result<u64, std::io::Error> {
    let mut n = 0;
    loop {
        let ch_data = reader.read_u8()? as u64;
        n = (n << 7) | (ch_data & 0x7f);
        if ch_data & 0x80 != 0 {
            n += 1;
        } else {
            return Ok(n);
        }
    }
}

fn decompress_amount(x: u64) -> u64 {
    if x == 0 {
        return 0;
    }

    let x = x - 1;
    let (mut x, e) = (x / 10, x % 10);
    let n = if e < 9 {
        let (x, d) =


