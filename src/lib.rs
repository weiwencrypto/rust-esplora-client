//! An extensible blocking/async Esplora client
//!
//! This library provides an extensible blocking and
//! async Esplora client to query Esplora's backend.
//!
//! The library provides the possibility to build a blocking
//! client using [`ureq`] and an async client using [`reqwest`].
//! The library supports communicating to Esplora via a proxy
//! and also using TLS (SSL) for secure communication.
//!
//!
//! ## Usage
//!
//! You can create a blocking client as follows:
//!
//! ```no_run
//! use esplora_client::Builder;
//! let builder = Builder::new("https://blockstream.info/testnet/api");
//! let blocking_client = builder.build_blocking();
//! # Ok::<(), esplora_client::Error>(());
//! ```
//!
//! Here is an example of how to create an asynchronous client.
//!
//! ```no_run
//! use esplora_client::Builder;
//! let builder = Builder::new("https://blockstream.info/testnet/api");
//! let async_client = builder.build_async();
//! # Ok::<(), esplora_client::Error>(());
//! ```
//!
//! ## Features
//!
//! By default the library enables all features. To specify
//! specific features, set `default-features` to `false` in your `Cargo.toml`
//! and specify the features you want. This will look like this:
//!
//! `esplora_client = { version = "*", default-features = false, features = ["blocking"] }`
//!
//! * `blocking` enables [`ureq`], the blocking client with proxy and TLS (SSL) capabilities.
//! * `async` enables [`reqwest`], the async client with proxy capabilities.
//! * `async-https` enables [`reqwest`], the async client with support for proxying and TLS (SSL).
//!
//!
use std::collections::HashMap;
use std::fmt;
use std::io;

use bitcoin::consensus;
use bitcoin::{BlockHash, Txid};

pub mod api;

#[cfg(any(feature = "async", feature = "async-https"))]
pub mod r#async;
#[cfg(feature = "blocking")]
pub mod blocking;

pub use api::*;
#[cfg(feature = "blocking")]
pub use blocking::BlockingClient;
#[cfg(any(feature = "async", feature = "async-https"))]
pub use r#async::AsyncClient;

/// Get a fee value in sats/vbytes from the estimates
/// that matches the confirmation target set as parameter.
pub fn convert_fee_rate(target: usize, estimates: HashMap<String, f64>) -> Result<f32, Error> {
    let fee_val = {
        let mut pairs = estimates
            .into_iter()
            .filter_map(|(k, v)| Some((k.parse::<usize>().ok()?, v)))
            .collect::<Vec<_>>();
        pairs.sort_unstable_by_key(|(k, _)| std::cmp::Reverse(*k));
        pairs
            .into_iter()
            .find(|(k, _)| k <= &target)
            .map(|(_, v)| v)
            .unwrap_or(1.0)
    };
    Ok(fee_val as f32)
}

#[derive(Debug, Clone)]
pub struct Builder {
    pub base_url: String,
    /// Optional URL of the proxy to use to make requests to the Esplora server
    ///
    /// The string should be formatted as: `<protocol>://<user>:<password>@host:<port>`.
    ///
    /// Note that the format of this value and the supported protocols change slightly between the
    /// blocking version of the client (using `ureq`) and the async version (using `reqwest`). For more
    /// details check with the documentation of the two crates. Both of them are compiled with
    /// the `socks` feature enabled.
    ///
    /// The proxy is ignored when targeting `wasm32`.
    pub proxy: Option<String>,
    /// Socket timeout.
    pub timeout: Option<u64>,
}

impl Builder {
    /// Instantiate a new builder
    pub fn new(base_url: &str) -> Self {
        Builder {
            base_url: base_url.to_string(),
            proxy: None,
            timeout: None,
        }
    }

    /// Set the proxy of the builder
    pub fn proxy(mut self, proxy: &str) -> Self {
        self.proxy = Some(proxy.to_string());
        self
    }

    /// Set the timeout of the builder
    pub fn timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// build a blocking client from builder
    #[cfg(feature = "blocking")]
    pub fn build_blocking(self) -> Result<BlockingClient, Error> {
        BlockingClient::from_builder(self)
    }

    // build an asynchronous client from builder
    #[cfg(feature = "async")]
    pub fn build_async(self) -> Result<AsyncClient, Error> {
        AsyncClient::from_builder(self)
    }
}

/// Errors that can happen during a sync with `Esplora`
#[derive(Debug)]
pub enum Error {
    /// Error during ureq HTTP request
    #[cfg(feature = "blocking")]
    Ureq(::ureq::Error),
    /// Transport error during the ureq HTTP call
    #[cfg(feature = "blocking")]
    UreqTransport(::ureq::Transport),
    /// Error during reqwest HTTP request
    #[cfg(any(feature = "async", feature = "async-https"))]
    Reqwest(::reqwest::Error),
    /// HTTP response error
    HttpResponse(u16),
    /// IO error during ureq response read
    Io(io::Error),
    /// No header found in ureq response
    NoHeader,
    /// Invalid number returned
    Parsing(std::num::ParseIntError),
    /// Invalid Bitcoin data returned
    BitcoinEncoding(bitcoin::consensus::encode::Error),
    /// Invalid Hex data returned
    Hex(bitcoin::hashes::hex::Error),

    /// Transaction not found
    TransactionNotFound(Txid),
    /// Header height not found
    HeaderHeightNotFound(u32),
    /// Header hash not found
    HeaderHashNotFound(BlockHash),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

macro_rules! impl_error {
    ( $from:ty, $to:ident ) => {
        impl_error!($from, $to, Error);
    };
    ( $from:ty, $to:ident, $impl_for:ty ) => {
        impl std::convert::From<$from> for $impl_for {
            fn from(err: $from) -> Self {
                <$impl_for>::$to(err)
            }
        }
    };
}

impl std::error::Error for Error {}
#[cfg(feature = "blocking")]
impl_error!(::ureq::Transport, UreqTransport, Error);
#[cfg(any(feature = "async", feature = "async-https"))]
impl_error!(::reqwest::Error, Reqwest, Error);
impl_error!(io::Error, Io, Error);
impl_error!(std::num::ParseIntError, Parsing, Error);
impl_error!(consensus::encode::Error, BitcoinEncoding, Error);
impl_error!(bitcoin::hashes::hex::Error, Hex, Error);

#[cfg(test)]
mod test {
    use super::*;
    use electrsd::{bitcoind, bitcoind::BitcoinD, ElectrsD};
    use lazy_static::lazy_static;
    use std::env;
    use tokio::sync::Mutex;
    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    use {
        bitcoin::hashes::Hash,
        bitcoin::Amount,
        electrsd::{
            bitcoind::bitcoincore_rpc::bitcoincore_rpc_json::AddressType,
            bitcoind::bitcoincore_rpc::RpcApi,
        },
        electrum_client::ElectrumApi,
        std::time::Duration,
        tokio::sync::OnceCell,
    };

    lazy_static! {
        static ref BITCOIND: BitcoinD = {
            let bitcoind_exe = env::var("BITCOIND_EXE")
                .ok()
                .or_else(|| bitcoind::downloaded_exe_path().ok())
                .expect(
                    "you need to provide an env var BITCOIND_EXE or specify a bitcoind version feature",
                );
            let conf = bitcoind::Conf::default();
            BitcoinD::with_conf(bitcoind_exe, &conf).unwrap()
        };
        static ref ELECTRSD: ElectrsD = {
            let electrs_exe = env::var("ELECTRS_EXE")
                .ok()
                .or_else(electrsd::downloaded_exe_path)
                .expect(
                    "you need to provide env var ELECTRS_EXE or specify an electrsd version feature",
                );
            let mut conf = electrsd::Conf::default();
            conf.http_enabled = true;
            ElectrsD::with_conf(electrs_exe, &BITCOIND, &conf).unwrap()
        };
        static ref MINER: Mutex<()> = Mutex::new(());
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    static PREMINE: OnceCell<()> = OnceCell::const_new();

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    async fn setup_clients() -> (BlockingClient, AsyncClient) {
        PREMINE
            .get_or_init(|| async {
                let _miner = MINER.lock().await;
                generate_blocks_and_wait(101);
            })
            .await;

        let esplora_url = ELECTRSD.esplora_url.as_ref().unwrap();

        let builder = Builder::new(&format!("http://{}", esplora_url));
        let blocking_client = builder.build_blocking().unwrap();

        let builder_async = Builder::new(&format!("http://{}", esplora_url));
        let async_client = builder_async.build_async().unwrap();

        (blocking_client, async_client)
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    fn generate_blocks_and_wait(num: usize) {
        let cur_height = BITCOIND.client.get_block_count().unwrap();
        generate_blocks(num);
        wait_for_block(cur_height as usize + num);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    fn generate_blocks(num: usize) {
        let address = BITCOIND
            .client
            .get_new_address(Some("test"), Some(AddressType::Legacy))
            .unwrap();
        let _block_hashes = BITCOIND
            .client
            .generate_to_address(num as u64, &address)
            .unwrap();
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    fn wait_for_block(min_height: usize) {
        let mut header = ELECTRSD.client.block_headers_subscribe().unwrap();
        loop {
            if header.height >= min_height {
                break;
            }
            header = exponential_backoff_poll(|| {
                ELECTRSD.trigger().unwrap();
                ELECTRSD.client.ping().unwrap();
                ELECTRSD.client.block_headers_pop().unwrap()
            });
        }
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    fn exponential_backoff_poll<T, F>(mut poll: F) -> T
    where
        F: FnMut() -> Option<T>,
    {
        let mut delay = Duration::from_millis(64);
        loop {
            match poll() {
                Some(data) => break data,
                None if delay.as_millis() < 512 => delay = delay.mul_f32(2.0),
                None => {}
            }

            std::thread::sleep(delay);
        }
    }

    #[test]
    fn feerate_parsing() {
        let esplora_fees = serde_json::from_str::<HashMap<String, f64>>(
            r#"{
  "25": 1.015,
  "5": 2.3280000000000003,
  "12": 2.0109999999999997,
  "15": 1.018,
  "17": 1.018,
  "11": 2.0109999999999997,
  "3": 3.01,
  "2": 4.9830000000000005,
  "6": 2.2359999999999998,
  "21": 1.018,
  "13": 1.081,
  "7": 2.2359999999999998,
  "8": 2.2359999999999998,
  "16": 1.018,
  "20": 1.018,
  "22": 1.017,
  "23": 1.017,
  "504": 1,
  "9": 2.2359999999999998,
  "14": 1.018,
  "10": 2.0109999999999997,
  "24": 1.017,
  "1008": 1,
  "1": 4.9830000000000005,
  "4": 2.3280000000000003,
  "19": 1.018,
  "144": 1,
  "18": 1.018
}
"#,
        )
        .unwrap();
        assert_eq!(convert_fee_rate(6, esplora_fees.clone()).unwrap(), 2.236);
        assert_eq!(
            convert_fee_rate(26, esplora_fees).unwrap(),
            1.015,
            "should inherit from value for 25"
        );
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_tx() {
        let (blocking_client, async_client) = setup_clients().await;

        let address = BITCOIND
            .client
            .get_new_address(Some("test"), Some(AddressType::Legacy))
            .unwrap();
        let txid = BITCOIND
            .client
            .send_to_address(
                &address,
                Amount::from_sat(1000),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let _miner = MINER.lock().await;
        generate_blocks_and_wait(1);

        let tx = blocking_client.get_tx(&txid).unwrap();
        let tx_async = async_client.get_tx(&txid).await.unwrap();
        assert_eq!(tx, tx_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_tx_no_opt() {
        let (blocking_client, async_client) = setup_clients().await;

        let address = BITCOIND
            .client
            .get_new_address(Some("test"), Some(AddressType::Legacy))
            .unwrap();
        let txid = BITCOIND
            .client
            .send_to_address(
                &address,
                Amount::from_sat(1000),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let _miner = MINER.lock().await;
        generate_blocks_and_wait(1);

        let tx_no_opt = blocking_client.get_tx_no_opt(&txid).unwrap();
        let tx_no_opt_async = async_client.get_tx_no_opt(&txid).await.unwrap();
        assert_eq!(tx_no_opt, tx_no_opt_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_tx_status() {
        let (blocking_client, async_client) = setup_clients().await;

        let address = BITCOIND
            .client
            .get_new_address(Some("test"), Some(AddressType::Legacy))
            .unwrap();
        let txid = BITCOIND
            .client
            .send_to_address(
                &address,
                Amount::from_sat(1000),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let _miner = MINER.lock().await;
        generate_blocks_and_wait(1);

        let tx_status = blocking_client.get_tx_status(&txid).unwrap().unwrap();
        let tx_status_async = async_client.get_tx_status(&txid).await.unwrap().unwrap();
        assert_eq!(tx_status, tx_status_async);
        assert!(tx_status.confirmed);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_header_by_hash() {
        let (blocking_client, async_client) = setup_clients().await;

        let block_hash = BITCOIND.client.get_block_hash(23).unwrap();

        let block_header = blocking_client.get_header_by_hash(&block_hash).unwrap();
        let block_header_async = async_client.get_header_by_hash(&block_hash).await.unwrap();
        assert_eq!(block_header, block_header_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_block_status() {
        let (blocking_client, async_client) = setup_clients().await;

        let block_hash = BITCOIND.client.get_block_hash(21).unwrap();
        let next_block_hash = BITCOIND.client.get_block_hash(22).unwrap();

        let expected = BlockStatus {
            in_best_chain: true,
            height: Some(21),
            next_best: Some(next_block_hash),
        };

        let block_status = blocking_client.get_block_status(&block_hash).unwrap();
        let block_status_async = async_client.get_block_status(&block_hash).await.unwrap();
        assert_eq!(expected, block_status);
        assert_eq!(expected, block_status_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_non_existing_block_status() {
        // Esplora returns the same status for orphaned blocks as for non-existing blocks:
        // non-existing: https://blockstream.info/api/block/0000000000000000000000000000000000000000000000000000000000000000/status
        // orphaned: https://blockstream.info/api/block/000000000000000000181b1a2354620f66868a723c0c4d5b24e4be8bdfc35a7f/status
        // (Here the block is cited as orphaned: https://bitcoinchain.com/block_explorer/block/000000000000000000181b1a2354620f66868a723c0c4d5b24e4be8bdfc35a7f/ )
        // For this reason, we only test for the non-existing case here.

        let (blocking_client, async_client) = setup_clients().await;

        let block_hash = BlockHash::all_zeros();

        let expected = BlockStatus {
            in_best_chain: false,
            height: None,
            next_best: None,
        };

        let block_status = blocking_client.get_block_status(&block_hash).unwrap();
        let block_status_async = async_client.get_block_status(&block_hash).await.unwrap();
        assert_eq!(expected, block_status);
        assert_eq!(expected, block_status_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_block_by_hash() {
        let (blocking_client, async_client) = setup_clients().await;

        let block_hash = BITCOIND.client.get_block_hash(21).unwrap();

        let expected = Some(BITCOIND.client.get_block(&block_hash).unwrap());

        let block = blocking_client.get_block_by_hash(&block_hash).unwrap();
        let block_async = async_client.get_block_by_hash(&block_hash).await.unwrap();
        assert_eq!(expected, block);
        assert_eq!(expected, block_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_block_by_hash_not_existing() {
        let (blocking_client, async_client) = setup_clients().await;

        let block = blocking_client
            .get_block_by_hash(&BlockHash::all_zeros())
            .unwrap();
        let block_async = async_client
            .get_block_by_hash(&BlockHash::all_zeros())
            .await
            .unwrap();
        assert!(block.is_none());
        assert!(block_async.is_none());
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_merkle_proof() {
        let (blocking_client, async_client) = setup_clients().await;

        let address = BITCOIND
            .client
            .get_new_address(Some("test"), Some(AddressType::Legacy))
            .unwrap();
        let txid = BITCOIND
            .client
            .send_to_address(
                &address,
                Amount::from_sat(1000),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let _miner = MINER.lock().await;
        generate_blocks_and_wait(1);

        let merkle_proof = blocking_client.get_merkle_proof(&txid).unwrap().unwrap();
        let merkle_proof_async = async_client.get_merkle_proof(&txid).await.unwrap().unwrap();
        assert_eq!(merkle_proof, merkle_proof_async);
        assert!(merkle_proof.pos > 0);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_output_status() {
        let (blocking_client, async_client) = setup_clients().await;

        let address = BITCOIND
            .client
            .get_new_address(Some("test"), Some(AddressType::Legacy))
            .unwrap();
        let txid = BITCOIND
            .client
            .send_to_address(
                &address,
                Amount::from_sat(1000),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let _miner = MINER.lock().await;
        generate_blocks_and_wait(1);

        let output_status = blocking_client
            .get_output_status(&txid, 1)
            .unwrap()
            .unwrap();
        let output_status_async = async_client
            .get_output_status(&txid, 1)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(output_status, output_status_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_height() {
        let (blocking_client, async_client) = setup_clients().await;
        let block_height = blocking_client.get_height().unwrap();
        let block_height_async = async_client.get_height().await.unwrap();
        assert!(block_height > 0);
        assert_eq!(block_height, block_height_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_tip_hash() {
        let (blocking_client, async_client) = setup_clients().await;
        let tip_hash = blocking_client.get_tip_hash().unwrap();
        let tip_hash_async = async_client.get_tip_hash().await.unwrap();
        assert_eq!(tip_hash, tip_hash_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_block_hash() {
        let (blocking_client, async_client) = setup_clients().await;

        let block_hash = BITCOIND.client.get_block_hash(21).unwrap();

        let block_hash_blocking = blocking_client.get_block_hash(21).unwrap();
        let block_hash_async = async_client.get_block_hash(21).await.unwrap();
        assert_eq!(block_hash, block_hash_blocking);
        assert_eq!(block_hash, block_hash_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_txid_at_block_index() {
        let (blocking_client, async_client) = setup_clients().await;

        let block_hash = BITCOIND.client.get_block_hash(23).unwrap();

        let txid_at_block_index = blocking_client
            .get_txid_at_block_index(&block_hash, 0)
            .unwrap()
            .unwrap();
        let txid_at_block_index_async = async_client
            .get_txid_at_block_index(&block_hash, 0)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(txid_at_block_index, txid_at_block_index_async);
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_get_fee_estimates() {
        let (blocking_client, async_client) = setup_clients().await;
        let fee_estimates = blocking_client.get_fee_estimates().unwrap();
        let fee_estimates_async = async_client.get_fee_estimates().await.unwrap();
        assert_eq!(fee_estimates.len(), fee_estimates_async.len());
    }

    #[cfg(all(feature = "blocking", any(feature = "async", feature = "async-https")))]
    #[tokio::test]
    async fn test_scripthash_txs() {
        let (blocking_client, async_client) = setup_clients().await;

        let address = BITCOIND
            .client
            .get_new_address(Some("test"), Some(AddressType::Legacy))
            .unwrap();
        let txid = BITCOIND
            .client
            .send_to_address(
                &address,
                Amount::from_sat(1000),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let _miner = MINER.lock().await;
        generate_blocks_and_wait(1);

        let expected_tx = BITCOIND
            .client
            .get_transaction(&txid, None)
            .unwrap()
            .transaction()
            .unwrap();
        let script = &expected_tx.output[0].script_pubkey;
        let scripthash_txs_txids: Vec<Txid> = blocking_client
            .scripthash_txs(script, None)
            .unwrap()
            .iter()
            .map(|tx| tx.txid)
            .collect();
        let scripthash_txs_txids_async: Vec<Txid> = async_client
            .scripthash_txs(script, None)
            .await
            .unwrap()
            .iter()
            .map(|tx| tx.txid)
            .collect();
        assert_eq!(scripthash_txs_txids, scripthash_txs_txids_async);
    }
}
