use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    style::{self, Stylize},
    terminal::{self, ClearType},
    queue,
};
use rand::Rng;
use std::sync::Arc;
use tokio::sync::Mutex;
use std::sync::Mutex as StdMutex;
use log::{debug, info, warn, error};
use simplelog::{Config, LevelFilter, WriteLogger};

// Ethers imports  
use ethers::{
    core::types::{H256, U256},
    middleware::SignerMiddleware,
    prelude::*,
    providers::{Http, Middleware, Provider},
    types::transaction::eip2718::TypedTransaction,
    types::transaction::eip1559::Eip1559TransactionRequest,
    signers::Signer,
};
use clap::Parser;
use dotenv::dotenv;

// Import our custom middleware for Rise
// Since we're in src/snake/onchain_snake.rs, we need to include the middleware from src/middleware/
#[path = "../middleware/mod.rs"]
mod middleware;
use middleware::sync_transaction::SyncTransactionMiddleware;

const BOARD_WIDTH: u16 = 20;
const BOARD_HEIGHT: u16 = 20;
const INITIAL_SPEED: u64 = 200;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Position {
    x: u16,
    y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn opposite(&self) -> Direction {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

#[derive(Debug, Clone)]
struct Snake {
    body: VecDeque<Position>,
    direction: Direction,
}

impl Snake {
    fn new(head: Position) -> Self {
        let mut body = VecDeque::new();
        // The head should be the first element
        body.push_back(head);
        // Then the body segments trailing behind
        body.push_back(Position { x: head.x.saturating_sub(1), y: head.y });
        body.push_back(Position { x: head.x.saturating_sub(2), y: head.y });
        
        Snake {
            body,
            direction: Direction::Right,
        }
    }
    
    fn change_direction(&mut self, new_direction: Direction) {
        if new_direction != self.direction.opposite() {
            self.direction = new_direction;
        }
    }
    
    fn move_forward(&mut self) -> Option<Position> {
        if let Some(&head) = self.body.front() {
            let new_head = match self.direction {
                Direction::Up => {
                    if head.y == 0 {
                        return None;
                    }
                    Position { x: head.x, y: head.y - 1 }
                },
                Direction::Down => {
                    if head.y >= BOARD_HEIGHT - 1 {
                        return None;
                    }
                    Position { x: head.x, y: head.y + 1 }
                },
                Direction::Left => {
                    if head.x == 0 {
                        return None;
                    }
                    Position { x: head.x - 1, y: head.y }
                },
                Direction::Right => {
                    if head.x >= BOARD_WIDTH - 1 {
                        return None;
                    }
                    Position { x: head.x + 1, y: head.y }
                },
            };
            
            // Check for self-collision
            if self.body.contains(&new_head) {
                return None;
            }
            
            self.body.push_front(new_head);
            self.body.pop_back();
            Some(new_head)
        } else {
            None
        }
    }
    
    fn grow(&mut self) {
        if let Some(&tail) = self.body.back() {
            self.body.push_back(tail);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TxStatus {
    Pending,
    Confirmed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TxMethod {
    Async,
    Rise,
}

impl std::fmt::Display for TxMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TxMethod::Async => write!(f, "async"),
            TxMethod::Rise => write!(f, "rise"),
        }
    }
}

impl std::str::FromStr for TxMethod {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "async" => Ok(TxMethod::Async),
            "rise" => Ok(TxMethod::Rise),
            _ => Err(format!("Invalid method: {}", s)),
        }
    }
}

struct BlockchainContext {
    client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
    nonce: Arc<Mutex<u64>>,
    gas_price: U256,
    method: TxMethod,
    sync_client: Option<SyncTransactionMiddleware<Arc<SignerMiddleware<Provider<Http>, LocalWallet>>>>,
    chain_id: u64,
}

#[derive(Debug, Clone)]
struct TransactionInfo {
    nonce: u64,
    hash: H256,
    status: TxStatus,
    timestamp: std::time::Instant,
    confirmation_time: Option<std::time::Duration>,
    direction: Option<Direction>,
    applied: bool,
}

struct Game {
    snake: Snake,
    food: Position,
    score: u32,
    game_over: bool,
    speed: u64,
    runtime_handle: tokio::runtime::Handle,
    blockchain_context: Arc<BlockchainContext>,
    transactions: Arc<StdMutex<Vec<TransactionInfo>>>,
    pending_direction: Arc<StdMutex<Option<Direction>>>,
    pending_moves_count: Arc<StdMutex<usize>>,
}

impl Game {
    fn new(blockchain_context: Arc<BlockchainContext>) -> Self {
        let mut game = Game {
            snake: Snake::new(Position { x: BOARD_WIDTH / 2, y: BOARD_HEIGHT / 2 }),
            food: Position { x: 0, y: 0 },
            score: 0,
            game_over: false,
            speed: INITIAL_SPEED,
            runtime_handle: tokio::runtime::Handle::current(),
            blockchain_context,
            transactions: Arc::new(StdMutex::new(Vec::new())),
            pending_direction: Arc::new(StdMutex::new(None)),
            pending_moves_count: Arc::new(StdMutex::new(0)),
        };
        game.spawn_food();
        game
    }
    
    fn spawn_food(&mut self) {
        let mut rng = rand::thread_rng();
        loop {
            let food = Position {
                x: rng.gen_range(0..BOARD_WIDTH),
                y: rng.gen_range(0..BOARD_HEIGHT),
            };
            
            if !self.snake.body.contains(&food) {
                self.food = food;
                break;
            }
        }
    }
    
    fn update(&mut self) {
        if self.game_over {
            return;
        }
        
        // Check for confirmed transactions and apply direction changes
        let mut direction_to_apply = None;
        let mut tx_nonce_to_mark = None;
        
        if let Ok(mut txs) = self.transactions.lock() {
            // Find the first confirmed transaction that hasn't been applied yet
            for tx in txs.iter_mut() {
                if tx.status == TxStatus::Confirmed && 
                   tx.direction.is_some() && 
                   !tx.applied {
                    direction_to_apply = tx.direction;
                    tx_nonce_to_mark = Some(tx.nonce);
                    break;
                }
            }
            
            // Mark the transaction as applied
            if let Some(nonce) = tx_nonce_to_mark {
                for tx in txs.iter_mut() {
                    if tx.nonce == nonce {
                        tx.applied = true;
                        break;
                    }
                }
            }
        }
        
        // Apply the direction if we found one
        if let Some(dir) = direction_to_apply {
            self.snake.change_direction(dir);
            // Decrement pending moves count
            if let Ok(mut count) = self.pending_moves_count.lock() {
                if *count > 0 {
                    *count -= 1;
                }
            }
        }
        
        match self.snake.move_forward() {
            Some(new_head) => {
                if new_head == self.food {
                    self.snake.grow();
                    self.score += 10;
                    self.spawn_food();
                    if self.score % 50 == 0 && self.speed > 50 {
                        self.speed -= 10;
                    }
                }
            }
            None => {
                self.game_over = true;
            }
        }
    }
    
    fn is_valid_move(&self, new_direction: Direction) -> bool {
        new_direction != self.snake.direction.opposite()
    }
    
    fn send_move_transaction(&self, direction: Direction) {
        // Check if we already have 4 pending moves
        if let Ok(count) = self.pending_moves_count.lock() {
            if *count >= 4 {
                debug!("Ignoring move - already have 4 pending moves");
                return;
            }
        }
        
        let blockchain_context = self.blockchain_context.clone();
        let transactions = self.transactions.clone();
        let pending_moves_count = self.pending_moves_count.clone();
        
        self.runtime_handle.spawn(async move {
            match Self::send_move_transaction_static(
                blockchain_context,
                direction,
                transactions,
                pending_moves_count,
            ).await {
                Ok(_) => {
                    debug!("Move transaction queued successfully");
                }
                Err(e) => {
                    error!("Failed to send move transaction: {}", e);
                }
            }
        });
    }
    
    // Send transaction without waiting for confirmation
    async fn send_move_transaction_static(
        blockchain_context: Arc<BlockchainContext>,
        direction: Direction,
        transactions: Arc<StdMutex<Vec<TransactionInfo>>>,
        pending_moves_count: Arc<StdMutex<usize>>,
    ) -> anyhow::Result<()> {
        let mut nonce = blockchain_context.nonce.lock().await;
        let current_nonce = *nonce;
        
        let client = &blockchain_context.client;
        let chain_id = blockchain_context.chain_id;
        
        // Capture start time
        let start_time = std::time::Instant::now();
        
        // Increment pending moves count
        {
            let mut count = pending_moves_count.lock().unwrap();
            *count += 1;
        }
        
        match blockchain_context.method {
            TxMethod::Rise => {
                // Use sendRawTransactionSync for Rise
                let max_priority_fee_per_gas = U256::from(1_000_000_000); // 1 gwei
                let max_fee_per_gas = if blockchain_context.gas_price > max_priority_fee_per_gas {
                    blockchain_context.gas_price
                } else {
                    max_priority_fee_per_gas * 2
                };
                
                let value = match direction {
                    Direction::Up => U256::from(1),
                    Direction::Down => U256::from(2),
                    Direction::Left => U256::from(3),
                    Direction::Right => U256::from(4),
                };
                
                // Create EIP-1559 transaction
                let tx_request = Eip1559TransactionRequest::new()
                    .from(client.address())
                    .to(client.address())
                    .value(value)
                    .chain_id(chain_id)
                    .nonce(current_nonce)
                    .gas(21000)
                    .max_fee_per_gas(max_fee_per_gas)
                    .max_priority_fee_per_gas(max_priority_fee_per_gas);
                
                let tx = TypedTransaction::Eip1559(tx_request);
                
                // Clone for the spawned task
                let client_clone = client.clone();
                let transactions_clone = transactions.clone();
                let pending_moves_count_clone = pending_moves_count.clone();
                let sync_client = blockchain_context.sync_client.clone().unwrap();
                
                // Spawn the transaction sending
                tokio::spawn(async move {
                    match Self::send_rise_transaction(
                        &client_clone,
                        &sync_client,
                        tx,
                        current_nonce,
                        direction,
                        start_time,
                        transactions_clone,
                        pending_moves_count_clone,
                    ).await {
                        Ok(_) => debug!("Rise transaction completed"),
                        Err(e) => error!("Rise transaction failed: {}", e),
                    }
                });
            },
            TxMethod::Async => {
                // Use regular async method
                let mut tx = TypedTransaction::default();
                tx.set_to(client.address());
                let value = match direction {
                    Direction::Up => U256::from(1),
                    Direction::Down => U256::from(2),
                    Direction::Left => U256::from(3),
                    Direction::Right => U256::from(4),
                };
                tx.set_value(value);
                tx.set_nonce(current_nonce);
                tx.set_gas(U256::from(21000));
                tx.set_gas_price(blockchain_context.gas_price);
                tx.set_chain_id(chain_id);
                
                // Clone for the spawned task
                let client_clone = client.clone();
                let transactions_clone = transactions.clone();
                let pending_moves_count_clone = pending_moves_count.clone();
                
                // Spawn the transaction sending
                tokio::spawn(async move {
                    match client_clone.send_transaction(tx, None).await {
                        Ok(pending_tx) => {
                            let tx_hash = pending_tx.tx_hash();
                            debug!("TX sent: hash={:?}, nonce={}", tx_hash, current_nonce);
                            
                            // Add transaction to tracking list
                            let tx_info = TransactionInfo {
                                nonce: current_nonce,
                                hash: tx_hash,
                                status: TxStatus::Pending,
                                timestamp: start_time,
                                confirmation_time: None,
                                direction: Some(direction),
                                applied: false,
                            };
                            
                            {
                                let mut txs = transactions_clone.lock().unwrap();
                                txs.push(tx_info);
                                if txs.len() > 10 {
                                    txs.remove(0);
                                }
                            }
                            
                            // Start monitoring for receipt
                            tokio::spawn(Self::monitor_transaction_receipt(
                                client_clone.clone(),
                                tx_hash,
                                current_nonce,
                                transactions_clone.clone(),
                                pending_moves_count_clone.clone(),
                                start_time,
                            ));
                        }
                        Err(e) => {
                            error!("Failed to send transaction: {}", e);
                            // Decrement pending moves count on error
                            if let Ok(mut count) = pending_moves_count_clone.lock() {
                                if *count > 0 {
                                    *count -= 1;
                                }
                            }
                        }
                    }
                });
            }
        }
        
        *nonce += 1;
        Ok(())
    }
    
    // Send Rise transaction using sendRawTransactionSync
    async fn send_rise_transaction(
        client: &Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
        sync_client: &SyncTransactionMiddleware<Arc<SignerMiddleware<Provider<Http>, LocalWallet>>>,
        tx: TypedTransaction,
        nonce: u64,
        direction: Direction,
        start_time: std::time::Instant,
        transactions: Arc<StdMutex<Vec<TransactionInfo>>>,
        pending_moves_count: Arc<StdMutex<usize>>,
    ) -> anyhow::Result<()> {
        // Sign the transaction
        let signature = client.signer().sign_transaction(&tx).await?;
        let raw_tx = tx.rlp_signed(&signature);
        
        // Send using sendRawTransactionSync
        match sync_client.send_raw_transaction_sync(raw_tx).await {
            Ok(receipt) => {
                let confirmation_time = start_time.elapsed();
                let tx_hash = receipt.transaction_hash;
                let status = if receipt.status == Some(1.into()) {
                    TxStatus::Confirmed
                } else {
                    TxStatus::Failed
                };
                
                // Add transaction to tracking list
                let tx_info = TransactionInfo {
                    nonce,
                    hash: tx_hash,
                    status,
                    timestamp: start_time,
                    confirmation_time: Some(confirmation_time),
                    direction: Some(direction),
                    applied: false,
                };
                
                {
                    let mut txs = transactions.lock().unwrap();
                    txs.push(tx_info);
                    if txs.len() > 10 {
                        txs.remove(0);
                    }
                }
                
                info!("TX confirmed (Rise): nonce={}, status={:?}, time={}ms", 
                     nonce, status, confirmation_time.as_millis());
                
                // If failed, decrement pending moves count
                if status == TxStatus::Failed {
                    if let Ok(mut count) = pending_moves_count.lock() {
                        if *count > 0 {
                            *count -= 1;
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to send Rise transaction: {}", e);
                // Decrement pending moves count on error
                if let Ok(mut count) = pending_moves_count.lock() {
                    if *count > 0 {
                        *count -= 1;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    // Monitor for transaction receipt (for async method only)
    async fn monitor_transaction_receipt(
        client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
        tx_hash: H256,
        nonce: u64,
        transactions: Arc<StdMutex<Vec<TransactionInfo>>>,
        pending_moves_count: Arc<StdMutex<usize>>,
        start_time: std::time::Instant,
    ) {
        for _ in 0..300 {  // Try for ~30 seconds
            match client.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => {
                    let confirmation_time = start_time.elapsed();
                    let status = if receipt.status == Some(1.into()) {
                        TxStatus::Confirmed
                    } else {
                        TxStatus::Failed
                    };
                    
                    // Update transaction status
                    if let Ok(mut txs) = transactions.lock() {
                        for tx in txs.iter_mut() {
                            if tx.nonce == nonce {
                                tx.status = status;
                                tx.confirmation_time = Some(confirmation_time);
                                info!("TX confirmed: nonce={}, status={:?}, time={}ms", 
                                     nonce, status, confirmation_time.as_millis());
                                break;
                            }
                        }
                    }
                    
                    // If failed, decrement pending moves count
                    if status == TxStatus::Failed {
                        if let Ok(mut count) = pending_moves_count.lock() {
                            if *count > 0 {
                                *count -= 1;
                            }
                        }
                    }
                    
                    return;
                }
                Ok(None) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(e) => {
                    warn!("Error checking receipt: {}", e);
                    // Decrement pending moves count on error
                    if let Ok(mut count) = pending_moves_count.lock() {
                        if *count > 0 {
                            *count -= 1;
                        }
                    }
                    return;
                }
            }
        }
        
        // Timeout - mark as failed and decrement pending moves count
        if let Ok(mut txs) = transactions.lock() {
            for tx in txs.iter_mut() {
                if tx.nonce == nonce {
                    tx.status = TxStatus::Failed;
                    tx.confirmation_time = Some(start_time.elapsed());
                    warn!("TX timeout: nonce={}", nonce);
                    break;
                }
            }
        }
        
        if let Ok(mut count) = pending_moves_count.lock() {
            if *count > 0 {
                *count -= 1;
            }
        }
    }
    
    fn draw(&self, stdout: &mut io::Stdout) -> anyhow::Result<()> {
        queue!(
            stdout,
            terminal::Clear(ClearType::All),
            cursor::Hide,
            cursor::MoveTo(0, 0)
        )?;
        
        let title = "ONCHAIN SNAKE GAME";
        let board_visual_width = (BOARD_WIDTH * 2) + 2;
        let title_x = if title.len() as u16 > board_visual_width {
            0
        } else {
            (board_visual_width - title.len() as u16) / 2
        };
        queue!(
            stdout,
            cursor::MoveTo(title_x, 0),
            style::Print(title.bold().cyan())
        )?;
        
        // Draw game board
        for x in 0..=BOARD_WIDTH * 2 + 1 {
            queue!(stdout, cursor::MoveTo(x, 1), style::Print("-"))?;
            queue!(stdout, cursor::MoveTo(x, BOARD_HEIGHT + 2), style::Print("-"))?;
        }
        
        for y in 1..=BOARD_HEIGHT + 2 {
            queue!(stdout, cursor::MoveTo(0, y), style::Print("|"))?;
            queue!(stdout, cursor::MoveTo(BOARD_WIDTH * 2 + 1, y), style::Print("|"))?;
        }
        
        queue!(stdout, cursor::MoveTo(0, 1), style::Print("+"))?;
        queue!(stdout, cursor::MoveTo(BOARD_WIDTH * 2 + 1, 1), style::Print("+"))?;
        queue!(stdout, cursor::MoveTo(0, BOARD_HEIGHT + 2), style::Print("+"))?;
        queue!(stdout, cursor::MoveTo(BOARD_WIDTH * 2 + 1, BOARD_HEIGHT + 2), style::Print("+"))?;
        
        // Draw snake
        for (i, segment) in self.snake.body.iter().enumerate() {
            let visual_x = segment.x * 2 + 1;
            queue!(
                stdout,
                cursor::MoveTo(visual_x, segment.y + 2),
                style::Print(if i == 0 { "@@" } else { "##" }.green())
            )?;
        }
        
        // Draw food
        let food_visual_x = self.food.x * 2 + 1;
        queue!(
            stdout,
            cursor::MoveTo(food_visual_x, self.food.y + 2),
            style::Print("**".red())
        )?;
        
        // Draw transaction list on the right
        let tx_list_x = board_visual_width + 5;
        queue!(
            stdout,
            cursor::MoveTo(tx_list_x, 1),
            style::Print("TRANSACTIONS".bold())
        )?;
        
        queue!(
            stdout,
            cursor::MoveTo(tx_list_x, 2),
            style::Print("Nonce | Status     | Time")
        )?;
        
        queue!(
            stdout,
            cursor::MoveTo(tx_list_x, 3),
            style::Print("------------------------")
        )?;
        
        if let Ok(txs) = self.transactions.lock() {
            for (i, tx) in txs.iter().enumerate() {
                let y = 4 + i as u16;
                if y > BOARD_HEIGHT + 2 {
                    break;
                }
                
                let status_str = match tx.status {
                    TxStatus::Pending => "Pending".yellow(),
                    TxStatus::Confirmed => "Confirmed".green(),
                    TxStatus::Failed => "Failed".red(),
                };
                
                let time_str = if let Some(time) = tx.confirmation_time {
                    format!("{}ms", time.as_millis())
                } else {
                    "-".to_string()
                };
                
                queue!(
                    stdout,
                    cursor::MoveTo(tx_list_x, y),
                    style::Print(format!("{:5} | ", tx.nonce)),
                    style::Print(status_str),
                    style::Print(format!(" | {}", time_str))
                )?;
            }
        }
        
        // Draw score and other info
        let info_y = BOARD_HEIGHT + 4;
        queue!(
            stdout,
            cursor::MoveTo(0, info_y),
            style::Print(format!("Score: {} | Speed: {} | Method: {}", 
                self.score, self.speed, self.blockchain_context.method))
        )?;
        
        // Draw pending moves count
        if let Ok(count) = self.pending_moves_count.lock() {
            queue!(
                stdout,
                cursor::MoveTo(0, info_y + 1),
                style::Print(format!("Pending Moves: {}/4", count))
            )?;
        }
        
        queue!(
            stdout,
            cursor::MoveTo(0, info_y + 2),
            style::Print("Controls: Arrow keys to move, Q to quit, R to reset")
        )?;
        
        if self.game_over {
            let game_over_msg = "GAME OVER!";
            let msg_x = (board_visual_width - game_over_msg.len() as u16) / 2;
            queue!(
                stdout,
                cursor::MoveTo(msg_x, BOARD_HEIGHT / 2),
                style::Print(game_over_msg.red().bold())
            )?;
            
            let msg = format!("Press R to restart");
            let msg_visual_x = (board_visual_width.saturating_sub(msg.len() as u16)) / 2;
            queue!(
                stdout,
                cursor::MoveTo(msg_visual_x, BOARD_HEIGHT / 2 + 1),
                style::Print(msg.red().on_black())
            )?;
        }
        
        stdout.flush()?;
        Ok(())
    }
    
    fn reset(&mut self) {
        self.snake = Snake::new(Position { x: BOARD_WIDTH / 2, y: BOARD_HEIGHT / 2 });
        self.score = 0;
        self.speed = INITIAL_SPEED;
        self.game_over = false;
        self.spawn_food();
        // Clear transaction list and reset pending moves count
        let transactions = self.transactions.clone();
        let pending_moves_count = self.pending_moves_count.clone();
        self.runtime_handle.spawn(async move {
            let mut txs = transactions.lock().unwrap();
            txs.clear();
            let mut count = pending_moves_count.lock().unwrap();
            *count = 0;
        });
    }
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, env = "RPC_PROVIDER")]
    rpc: Option<String>,
    
    #[arg(short, long, env = "PRIVATE_KEY")]
    pkey: Option<String>,
    
    #[arg(short, long, value_enum, default_value = "async")]
    method: TxMethod,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    
    // Set up logging to debug.log
    let log_file = std::fs::File::create("debug.log")?;
    WriteLogger::init(LevelFilter::Debug, Config::default(), log_file)?;
    
    info!("Starting onchain snake game");
    debug!("Logging initialized to debug.log");
    
    let args = Args::parse();
    
    let rpc_url = args.rpc.expect("RPC_PROVIDER must be set either via --rpc or environment variable");
    let private_key = args.pkey.expect("PRIVATE_KEY must be set either via --pkey or environment variable");
    
    // Auto-detect if we should use rise method based on RPC URL
    let method = if rpc_url.to_lowercase().contains("rise") {
        info!("RPC URL contains 'rise', automatically using Rise method");
        TxMethod::Rise
    } else {
        args.method
    };
    
    info!("Initializing blockchain connection...");
    
    let provider = Provider::<Http>::try_from(&rpc_url)?;
    let wallet: LocalWallet = private_key.parse()?;
    let wallet_address = wallet.address();
    let chain_id = provider.get_chainid().await?;
    let wallet = wallet.with_chain_id(chain_id.as_u64());
    
    let client = Arc::new(SignerMiddleware::new(provider, wallet));
    
    // Create sync client if using rise method
    let sync_client = match method {
        TxMethod::Rise => Some(SyncTransactionMiddleware::new(client.clone())),
        _ => None,
    };
    
    let starting_nonce = client.get_transaction_count(wallet_address, None).await?.as_u64();
    let gas_price = client.get_gas_price().await?;
    let gas_price = if gas_price.is_zero() {
        U256::from(1_000_000_000) // 1 gwei
    } else {
        gas_price * 2 // 2x default
    };
    debug!("Raw gas price: {}, Using: {}", client.get_gas_price().await?, gas_price);
    
    info!("Connected to {} (chain ID: {})", rpc_url, chain_id);
    info!("Wallet: {}", wallet_address);
    info!("Starting nonce: {}", starting_nonce);
    info!("Gas price: {} gwei", gas_price / 1_000_000_000);
    info!("Method: {:?}", method);
    info!("Starting onchain snake game...");
    
    // Check wallet balance
    let balance = client.get_balance(wallet_address, None).await?;
    info!("Wallet balance: {} ETH", balance / U256::exp10(18));
    
    let blockchain_context = Arc::new(BlockchainContext {
        client: client.clone(),
        nonce: Arc::new(Mutex::new(starting_nonce)),
        gas_price,
        method,
        sync_client,
        chain_id: chain_id.as_u64(),
    });
    
    let mut stdout = io::stdout();
    
    terminal::enable_raw_mode()?;
    execute!(stdout, terminal::EnterAlternateScreen)?;
    
    let mut game = Game::new(blockchain_context);
    
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        loop {
            if event::poll(Duration::from_millis(10)).unwrap() {
                if let Event::Key(key_event) = event::read().unwrap() {
                    tx.send(key_event).unwrap();
                }
            }
        }
    });
    
    let mut last_update = std::time::Instant::now();
    
    loop {
        // Draw the game
        game.draw(&mut stdout)?;
        
        // Check for input
        if let Ok(key_event) = rx.try_recv() {
            match key_event.code {
                KeyCode::Char('q') | KeyCode::Char('Q') => break,
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    game.reset();
                },
                KeyCode::Up => {
                    if !game.game_over {
                        game.send_move_transaction(Direction::Up);
                    }
                },
                KeyCode::Down => {
                    if !game.game_over {
                        game.send_move_transaction(Direction::Down);
                    }
                },
                KeyCode::Left => {
                    if !game.game_over {
                        game.send_move_transaction(Direction::Left);
                    }
                },
                KeyCode::Right => {
                    if !game.game_over {
                        game.send_move_transaction(Direction::Right);
                    }
                },
                _ => {}
            }
        }
        
        // Update game state
        if !game.game_over && last_update.elapsed() >= Duration::from_millis(game.speed) {
            game.update();
            last_update = std::time::Instant::now();
        }
        
        thread::sleep(Duration::from_millis(10));
    }
    
    execute!(stdout, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    
    Ok(())
}