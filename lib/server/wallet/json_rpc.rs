use std::borrow::Cow;

use bitcoin::{
    Amount, BlockHash, Transaction, Txid, absolute,
    consensus::Decodable as _,
    consensus::encode::VarInt,
    hashes::{Hash as _, sha256d},
    transaction,
};
use futures::TryFutureExt as _;
use jsonrpsee::{
    core::{RpcResult, async_trait},
    proc_macros::rpc,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    messages,
    server::custom_json_rpc_err,
    types::{BmmCommitment, SidechainDeclaration, SidechainNumber, SidechainProposal},
    wallet::SidechainDepositTransaction,
};

#[derive(Debug, Error)]
#[error("BMM request with same sidechain number and previous block hash already exists")]
struct BmmRequestAlreadyExistsError;

fn deserialize_transaction_or_legacy_zero_input(
    transaction_bytes: &[u8],
) -> Result<Transaction, bitcoin::consensus::encode::Error> {
    match bitcoin::consensus::deserialize(transaction_bytes) {
        Ok(transaction) => Ok(transaction),
        Err(original_err) => {
            let mut cursor = transaction_bytes;
            let Ok(version) = transaction::Version::consensus_decode(&mut cursor) else {
                return Err(original_err);
            };
            let Ok(input_count) = VarInt::consensus_decode(&mut cursor) else {
                return Err(original_err);
            };
            if input_count.0 != 0 {
                return Err(original_err);
            }
            let Ok(output) = Vec::<bitcoin::TxOut>::consensus_decode(&mut cursor) else {
                return Err(original_err);
            };
            let Ok(lock_time) = absolute::LockTime::consensus_decode(&mut cursor) else {
                return Err(original_err);
            };
            if !cursor.is_empty() {
                return Err(original_err);
            }
            Ok(Transaction {
                version,
                lock_time,
                input: Vec::new(),
                output,
            })
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateSidechainProposalParams {
    pub sidechain_id: SidechainNumber,
    pub title: String,
    pub description: String,
    pub hash_id_1: String,
    pub hash_id_2: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreateSidechainProposalResult {
    pub sidechain_number: SidechainNumber,
    pub description: String,
    pub description_sha256d_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateDepositTransactionParams {
    pub sidechain_id: SidechainNumber,
    pub address: String,
    pub value_sats: u64,
    pub fee_sats: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreateDepositTransactionResult {
    pub txid: Txid,
}

#[derive(Debug, Deserialize)]
pub struct BroadcastWithdrawalBundleParams {
    pub sidechain_id: SidechainNumber,
    pub transaction: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BroadcastWithdrawalBundleResult {
    pub m6id: Txid,
}

#[derive(Debug, Error)]
enum CreateSidechainProposalJsonError {
    #[error("hash_id_1 must be 32 bytes of hex")]
    HashId1,
    #[error("hash_id_2 must be 20 bytes of hex")]
    HashId2,
    #[error(transparent)]
    PushBytes(#[from] bitcoin::script::PushBytesError),
    #[error(transparent)]
    Persistence(#[from] rusqlite::Error),
}

#[derive(Debug, Error)]
enum CreateDepositTransactionJsonError {
    #[error("sidechain {0} is not active")]
    SidechainNotActive(SidechainNumber),
    #[error("address must be non-empty")]
    EmptyAddress,
    #[error("value_sats must be greater than zero")]
    ZeroValue,
    #[error(transparent)]
    CreateDeposit(#[from] crate::wallet::error::CreateDeposit),
    #[error(transparent)]
    GetSidechains(#[from] crate::validator::GetSidechainsError),
}

#[derive(Debug, Error)]
enum BroadcastWithdrawalBundleJsonError {
    #[error("transaction must be hex")]
    TransactionHex(#[source] hex::FromHexError),
    #[error("transaction consensus decode failed")]
    TransactionDecode(#[source] bitcoin::consensus::encode::Error),
    #[error(transparent)]
    BlindedM6(#[from] crate::types::BlindedM6Error),
    #[error(transparent)]
    Persistence(#[from] rusqlite::Error),
}

#[rpc(namespace = "wallet", namespace_separator = ".", server)]
pub trait Rpc {
    #[method(name = "create_sidechain_proposal")]
    async fn create_sidechain_proposal(
        &self,
        params: CreateSidechainProposalParams,
    ) -> RpcResult<CreateSidechainProposalResult>;

    #[method(name = "create_deposit_transaction")]
    async fn create_deposit_transaction(
        &self,
        params: CreateDepositTransactionParams,
    ) -> RpcResult<CreateDepositTransactionResult>;

    #[method(name = "list_sidechain_deposit_transactions")]
    async fn list_sidechain_deposit_transactions(
        &self,
    ) -> RpcResult<Vec<SidechainDepositTransaction>>;

    #[method(name = "broadcast_withdrawal_bundle")]
    async fn broadcast_withdrawal_bundle(
        &self,
        params: BroadcastWithdrawalBundleParams,
    ) -> RpcResult<BroadcastWithdrawalBundleResult>;

    #[method(name = "create_bmm_critical_data_transaction")]
    async fn create_bmm_critical_data_transaction(
        &self,
        sidechain_id: SidechainNumber,
        value_sats: u64,
        lock_time: bitcoin::absolute::LockTime,
        critical_hash: BmmCommitment,
        prev_block_hash: BlockHash,
    ) -> RpcResult<Txid>;
}

#[async_trait]
impl RpcServer for crate::wallet::Wallet {
    async fn create_sidechain_proposal(
        &self,
        params: CreateSidechainProposalParams,
    ) -> RpcResult<CreateSidechainProposalResult> {
        let hash_id_1 = hex::decode(params.hash_id_1)
            .ok()
            .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
            .ok_or(CreateSidechainProposalJsonError::HashId1)
            .map_err(custom_json_rpc_err)?;
        let hash_id_2 = hex::decode(params.hash_id_2)
            .ok()
            .and_then(|bytes| <[u8; 20]>::try_from(bytes).ok())
            .ok_or(CreateSidechainProposalJsonError::HashId2)
            .map_err(custom_json_rpc_err)?;
        let declaration = SidechainDeclaration {
            title: params.title,
            description: params.description,
            hash_id_1,
            hash_id_2,
        };
        let (_txout, description) =
            messages::create_sidechain_proposal(params.sidechain_id, &declaration)
                .map_err(CreateSidechainProposalJsonError::from)
                .map_err(custom_json_rpc_err)?;
        let proposal = SidechainProposal {
            sidechain_number: params.sidechain_id,
            description,
        };
        self.propose_sidechain(&proposal)
            .await
            .map_err(CreateSidechainProposalJsonError::from)
            .map_err(custom_json_rpc_err)?;
        let description_hash = proposal.description.sha256d_hash();
        Ok(CreateSidechainProposalResult {
            sidechain_number: proposal.sidechain_number,
            description: hex::encode(&proposal.description.0),
            description_sha256d_hash: hex::encode(sha256d::Hash::to_byte_array(description_hash)),
        })
    }

    async fn list_sidechain_deposit_transactions(
        &self,
    ) -> RpcResult<Vec<SidechainDepositTransaction>> {
        self.list_sidechain_deposit_transactions()
            .map_err(custom_json_rpc_err)
            .await
    }

    async fn broadcast_withdrawal_bundle(
        &self,
        params: BroadcastWithdrawalBundleParams,
    ) -> RpcResult<BroadcastWithdrawalBundleResult> {
        let transaction_bytes = hex::decode(params.transaction)
            .map_err(BroadcastWithdrawalBundleJsonError::TransactionHex)
            .map_err(custom_json_rpc_err)?;
        let transaction: Transaction =
            deserialize_transaction_or_legacy_zero_input(&transaction_bytes)
                .map_err(BroadcastWithdrawalBundleJsonError::TransactionDecode)
                .map_err(custom_json_rpc_err)?;
        let blinded_m6 = crate::types::BlindedM6::try_from(Cow::Owned(transaction))
            .map_err(BroadcastWithdrawalBundleJsonError::from)
            .map_err(custom_json_rpc_err)?
            .into_owned();
        let m6id = self
            .put_withdrawal_bundle(params.sidechain_id, &blinded_m6)
            .await
            .map_err(BroadcastWithdrawalBundleJsonError::from)
            .map_err(custom_json_rpc_err)?;

        Ok(BroadcastWithdrawalBundleResult { m6id: m6id.0 })
    }

    async fn create_deposit_transaction(
        &self,
        params: CreateDepositTransactionParams,
    ) -> RpcResult<CreateDepositTransactionResult> {
        if params.address.is_empty() {
            return Err(custom_json_rpc_err(
                CreateDepositTransactionJsonError::EmptyAddress,
            ));
        }

        let value = Amount::from_sat(params.value_sats);
        if value == Amount::ZERO {
            return Err(custom_json_rpc_err(
                CreateDepositTransactionJsonError::ZeroValue,
            ));
        }

        if !self
            .is_sidechain_active(params.sidechain_id)
            .map_err(CreateDepositTransactionJsonError::from)
            .map_err(custom_json_rpc_err)?
        {
            return Err(custom_json_rpc_err(
                CreateDepositTransactionJsonError::SidechainNotActive(params.sidechain_id),
            ));
        }

        let fee = params.fee_sats.map(Amount::from_sat);
        let txid = self
            .create_deposit(params.sidechain_id, params.address, value, fee)
            .await
            .map_err(CreateDepositTransactionJsonError::from)
            .map_err(custom_json_rpc_err)?;

        Ok(CreateDepositTransactionResult { txid })
    }

    async fn create_bmm_critical_data_transaction(
        &self,
        sidechain_id: SidechainNumber,
        value_sats: u64,
        lock_time: bitcoin::absolute::LockTime,
        critical_hash: BmmCommitment,
        prev_block_hash: BlockHash,
    ) -> RpcResult<Txid> {
        let amount = bdk_wallet::bitcoin::Amount::from_sat(value_sats);
        let tx = self
            .create_bmm_request(
                sidechain_id,
                prev_block_hash,
                critical_hash,
                amount,
                lock_time,
            )
            .await
            .map_err(custom_json_rpc_err)?
            .ok_or_else(|| custom_json_rpc_err(BmmRequestAlreadyExistsError))?;
        Ok(tx.compute_txid())
    }
}
