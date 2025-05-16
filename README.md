# EVM TX Latency Test

A Rust-based benchmark tool for measuring transaction submission and confirmation latency on EVM-compatible blockchains. This test aims to test a typical users experience when submitting a transactions to an EVM blockchain.

## Description

This is a very simple tool that sends 10 sequential transactions to a specified RPC endpoint and measures the time taken for each transaction to be sent and confirmed. It can be used to test the performance of different RPC providers or to benchmark the latency of your own node or a network. 

The benchmark supports three different transaction submission methods:
- `async`: Standard asynchronous transaction submission and receipt request (default)
- `rise`: Uses `eth_sendRawTransactionSync` for synchronous transaction submission
- `mega`: Uses `realtime_sendRawTransaction` for realtime transaction processing

## Prerequisites

- Rust and Cargo
- An EVM-compatible blockchain endpoint (RPC URL)
- A funded wallet for the target blockchain

## Installation

1. Clone the repository:
```
git clone https://github.com/yourusername/tx-latency.git
cd tx-latency
```

2. Build the project:
```
cargo build --release
```

## Configuration

You can configure the tool using environment variables or command line arguments:

Create a `.env` file in the project root with the following variables:

```
RPC_PROVIDER=https://your-rpc-endpoint.com
PRIVATE_KEY=your_wallet_private_key
```

Replace the values with your own RPC endpoint and private key.

## Usage

After building the project, you can run the benchmark in two ways:

### 1. Using the executable directly:

```
./target/release/tx-latency [OPTIONS]
```

### 2. Using cargo run with the `--` separator:

```
cargo run --release -- [OPTIONS]
```

Options:
- `-t, --type`: Transaction submission method (`async`, `rise`, or `mega`). Default is `async`.
- `-n, --num`: Number of transactions to send. Default is 10.
- `--rpc`: RPC endpoint URL. Defaults to the RPC_PROVIDER environment variable.
- `--pkey`: Private key for the wallet. Defaults to the PRIVATE_KEY environment variable.

Examples:

```bash
# Run the default benchmark with 10 transactions using environment variables
./target/release/tx-latency --type async --num 15 --rpc https://my-rpc.com --pkey 0x123456...

# Using cargo run with the -- separator
cargo run --release -- --num 20
```

## Output

The tool outputs detailed information for each transaction, including:

- Transaction hash
- Send time
- Confirmation time
- Total processing time
- Block information

After all transactions are completed, it displays a summary with statistical information including minimum, maximum, and average latency metrics for send, confirm, and total transaction times.