use anyhow::Result;
use ethers::{
    core::types::Bytes,
    middleware::{Middleware, MiddlewareError},
    providers::JsonRpcClient,
    types::TransactionReceipt,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RealtimeMiddlewareError<M: Middleware> {
    #[error("Middleware error: {0}")]
    MiddlewareError(M::Error),

    #[error("RPC error: {0}")]
    RpcError(String),
}

impl<M: Middleware> MiddlewareError for RealtimeMiddlewareError<M> {
    type Inner = M::Error;

    fn from_err(src: M::Error) -> Self {
        Self::MiddlewareError(src)
    }

    fn as_inner(&self) -> Option<&Self::Inner> {
        match self {
            Self::MiddlewareError(e) => Some(e),
            _ => None,
        }
    }
}

/// RealtimeTransactionMiddleware provides access to the `realtime_sendRawTransaction` RPC method
/// which both sends and waits for transaction receipt in a single call
#[derive(Debug)]
pub struct RealtimeTransactionMiddleware<M> {
    inner: M,
}

impl<M> RealtimeTransactionMiddleware<M>
where
    M: Middleware,
{
    /// Create a new instance of the RealtimeTransactionMiddleware
    pub fn new(inner: M) -> Self {
        Self { inner }
    }

    /// Send a raw transaction using the `realtime_sendRawTransaction` RPC method
    /// which returns a receipt directly in a single HTTP call
    pub async fn send_raw_transaction_realtime(
        &self,
        raw_tx: Bytes,
    ) -> Result<TransactionReceipt, RealtimeMiddlewareError<M>>
    where
        M: Middleware,
        M::Provider: JsonRpcClient,
    {
        let provider = self.inner.provider();
        
        // Ensure the byte sequence is properly prefixed according to EIP-2718 format
        let hex_value = format!("0x{}", hex::encode(&raw_tx));
        let params = [serde_json::Value::String(hex_value)];
        
        provider
            .request("realtime_sendRawTransaction", params)
            .await
            .map_err(|e| RealtimeMiddlewareError::RpcError(e.to_string()))
    }
}

// Implement Middleware trait so it can be used in middleware chain
impl<M> Middleware for RealtimeTransactionMiddleware<M>
where
    M: Middleware,
{
    type Error = RealtimeMiddlewareError<M>;
    type Provider = M::Provider;
    type Inner = M;

    fn inner(&self) -> &M {
        &self.inner
    }
}
