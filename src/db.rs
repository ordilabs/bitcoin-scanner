use postgres::{Client, Error, NoTls};

#[derive(Debug)]
pub struct InscriptionRecord {
    pub _id: i32,
    pub commit_output_script: Vec<u8>,
    pub txid: [u8; 32],
    pub index: usize,
    pub genesis_inscribers: Vec<[u8; 32]>,
    pub genesis_amount: u64,
    pub address: String,
    pub content_length: usize,
    pub content_type: String,
    pub genesis_block_hash: [u8; 32],
    pub genesis_fee: u64,
    pub genesis_height: u32,
    pub short_input_id: i64,
    pub offset: u64, // named tx_offset in the DB because it is a reserved word
    // pub ssn: u64, // Need ord/full blockchain context
    pub digest: [u8; 32],
}

pub struct SatsNameRecord {
    pub _id: i32,
    pub inscription_record_id: i32,
    pub short_input_id: i64,
    pub name: String,
}

pub struct DB {
    client: Client,
}

impl DB {
    pub fn setup(reset: bool) -> Result<Self, Error> {
        let mut client =
            Client::connect("postgresql://orduser:testtest@localhost/ordscanner", NoTls)?;

        if reset {
            client.batch_execute("DROP TABLE IF EXISTS sats_name;")?;
            client.batch_execute("DROP TABLE IF EXISTS inscription_record;")?;
        }

        client.batch_execute(
            "
            CREATE TABLE IF NOT EXISTS inscription_record (
                id                       SERIAL PRIMARY KEY,
                commit_output_script     BYTEA NOT NULL,
                txid                     BYTEA NOT NULL,
                index                    INTEGER NOT NULL,
                genesis_inscribers       BYTEA[] NOT NULL,
                genesis_amount           BIGINT NOT NULL,
                address                  VARCHAR NOT NULL,
                content_length           BIGINT NOT NULL,
                content_type             VARCHAR NOT NULL,
                genesis_block_hash       BYTEA NOT NULL,
                genesis_fee              BIGINT NOT NULL,
                genesis_height           INTEGER NOT NULL,
                short_input_id           BIGINT NOT NULL,
                tx_offset                BIGINT NOT NULL,
                digest                   BYTEA NOT NULL
                )
        ",
        )?;

        client.batch_execute(
            "
            CREATE TABLE IF NOT EXISTS sats_name (
                id                       SERIAL PRIMARY KEY,
                inscription_record_id    INTEGER NOT NULL REFERENCES inscription_record(id),
                short_input_id           BIGINT NOT NULL,
                name                     VARCHAR NOT NULL
                )
        ",
        )?;

        if client.is_closed() {
            println!("Client is not connected.");
        } else {
            println!("Client is connected.");
        }

        Ok(DB { client })
    }

    pub async fn insert_inscription(&mut self, r: &InscriptionRecord) -> Result<i32, Error> {
        let stmt = self
            .client
            .prepare("INSERT INTO inscription_record (commit_output_script, txid, index, genesis_inscribers, genesis_amount, address, content_length, content_type, genesis_block_hash, genesis_fee, genesis_height, short_input_id, tx_offset, digest) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14) RETURNING id");

        let stmt = match stmt {
            Ok(s) => s,
            Err(e) => return Err(e),
        };

        let inscribers: Vec<&[u8]> = r
            .genesis_inscribers
            .iter()
            .map(|array| array.as_ref())
            .collect();

        match self.client.query_one(
            &stmt,
            &[
                &r.commit_output_script,
                &r.txid.to_vec(),
                &(r.index as i32),
                &inscribers,
                &(r.genesis_amount as i64),
                &r.address,
                &(r.content_length as i64),
                &r.content_type,
                &r.genesis_block_hash.to_vec(),
                &(r.genesis_fee as i64),
                &(r.genesis_height as i32),
                &r.short_input_id,
                &(r.offset as i64),
                &r.digest.to_vec(),
            ],
        ) {
            Ok(row) => {
                let id: i32 = row.get(0);
                Ok(id)
            }
            Err(err) => {
                println!("Error: {:?}", err);
                Err(err)
            }
        }
    }

    pub async fn insert_sats_name(&mut self, r: &SatsNameRecord) -> Result<u64, Error> {
        let stmt = self
            .client
            .prepare("INSERT INTO sats_name (inscription_record_id, short_input_id, name) VALUES ($1, $2, $3)");

        let stmt = match stmt {
            Ok(s) => s,
            Err(e) => return Err(e),
        };

        match self.client.execute(
            &stmt,
            &[&r.inscription_record_id, &r.short_input_id, &r.name],
        ) {
            Ok(rows_affected) => Ok(rows_affected),
            Err(err) => {
                println!("Error: {:?}", err);
                Err(err)
            }
        }
    }
}
