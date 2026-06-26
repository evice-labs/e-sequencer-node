use std::{
    collections::HashMap,
    io::Write,
    path::Path,
    sync::{atomic::{AtomicBool, Ordering}, Arc},
    time::Duration,
    fs,
};

use clap::Parser;
use libp2p::identity::{ed25519, Keypair as P2pKeypair};
use libp2p::PeerId;
use log::{error, info, warn};
use rand::RngExt;
use schnorrkel::SecretKey as SchnorrkelSecretKey;

use evice_sequencer::{
    consensus::{ConsensusEngine, ConsensusState, QuorumCertificate},
    crypto::{public_key_to_address, KeyPair, ValidatorKeys},
    genesis::Genesis,
    keystore::Keystore,
    p2p::{self, AddressBook, P2pCommand, SyncResponse},
    AppPayload, ChainMessage, PayloadBatch,
};

use tokio::sync::{mpsc, Mutex, RwLock};

// CLI Arguments 
#[derive(Parser, Debug)]
#[clap(name = "e-sequencer-node", version, about = "Evice Sequencer Node")]
struct Args {
    #[clap(long)]
    bootstrap: bool,
    #[clap(long, default_value = "4")]
    num_validators: usize,
    #[clap(long)]
    bootstrap_node: Vec<String>,
    #[clap(long, default_value = "9000", env = "P2P_PORT")]
    p2p_port: u16,
    #[clap(long, default_value = "genesis.json", env = "GENESIS_PATH")]
    genesis: String,
    #[clap(long, default_value = "./node_data", env = "DATA_DIR")]
    data_dir: String,
    #[clap(long, env = "KEYSTORE_PATH")]
    keystore_path: Option<String>,
    #[clap(long, env = "VRF_PRIV_KEY")]
    vrf_priv_key: Option<String>,
    #[clap(long, env = "KEYSTORE_PASSWORD")]
    password: Option<String>,
    #[clap(long, env = "DEV_MODE")]
    dev: bool,
    #[clap(long)]
    get_peer_id: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    if args.dev {
        p2p::DEV_MODE.store(true, Ordering::SeqCst);
        warn!("[MAIN] Running in DEVELOPMENT mode. Loopback P2P addresses are allowed.");
    }

    if args.get_peer_id {
        let keypair = load_or_create_p2p_keypair(&args.data_dir)?;
        let peer_id = PeerId::from(keypair.public());
        println!("{}", peer_id);
        return Ok(());
    }

    if args.bootstrap {
        return run_bootstrap(args.num_validators, &args.data_dir);
    }

    run_node(args).await
}

// Bootstrap: Generate Genesis + Validator Keys 
fn run_bootstrap(
    num_validators: usize,
    data_dir: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!(
        "[BOOTSTRAP] Generating genesis.json for {} validators...",
        num_validators
    );

    let mut validator_keys_all: Vec<ValidatorKeys> = Vec::new();
    let mut p2p_keypairs: Vec<P2pKeypair> = Vec::new();
    let mut genesis_accounts = HashMap::new();

    for _ in 0..num_validators {
        validator_keys_all.push(ValidatorKeys::new());
        p2p_keypairs.push(P2pKeypair::generate_ed25519());
    }

    for (i, keys) in validator_keys_all.iter().enumerate() {
        let pub_key_hex = hex::encode(keys.signing_keys.public_key_bytes());
        let address = public_key_to_address(&keys.signing_keys.public_key_bytes());
        let address_hex = format!("0x{}", hex::encode(address.as_ref()));

        let peer_id = PeerId::from(p2p_keypairs[i].public());
        let port = 9000 + i as u16;
        let multiaddr = format!("/ip4/127.0.0.1/tcp/{}/p2p/{}", port, peer_id);

        let account = evice_sequencer::genesis::GenesisAccount {
            public_key: pub_key_hex,
            vrf_public_key: Some(hex::encode(keys.vrf_keys.public.to_bytes())),
            network_identity: Some(multiaddr),
        };
        genesis_accounts.insert(address_hex, account);
    }

    let genesis = Genesis {
        genesis_time: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        chain_id: "evice-sequencer-v1".to_string(),
        parameters: evice_sequencer::genesis::GenesisParameters {
            sub_committee_size: num_validators,
            proposer_timeout_ms: 3000,
        },
        accounts: genesis_accounts,
    };

    let genesis_json = serde_json::to_string_pretty(&genesis)?;
    let mut file = std::fs::File::create("genesis.json")?;
    file.write_all(genesis_json.as_bytes())?;

    // Save individual validator keys for reference
    println!("\n══════════════════════════════════════════════════════════════");
    println!("             GENERATED VALIDATOR KEYS                        ");
    println!("══════════════════════════════════════════════════════════════");
    for (i, keys) in validator_keys_all.iter().enumerate() {
        let address = public_key_to_address(&keys.signing_keys.public_key_bytes());
        println!("\n─── Validator {} ───", i + 1);
        println!("  Address:         0x{}", hex::encode(address.as_ref()));
        println!(
            "  Signing PubKey:  0x{}",
            hex::encode(keys.signing_keys.public_key_bytes())
        );
        println!(
            "  Signing PrivKey: 0x{}",
            hex::encode(keys.signing_keys.private_key_bytes())
        );
        println!(
            "  VRF PubKey:      0x{}",
            hex::encode(keys.vrf_keys.public.to_bytes())
        );
        println!(
            "  VRF PrivKey:     0x{}",
            hex::encode(keys.vrf_keys.secret.to_bytes())
        );
        println!(
            "  P2P PeerId:      {}",
            PeerId::from(p2p_keypairs[i].public())
        );
        println!("  P2P Port:        {}", 9000 + i);

        // Save P2P keypair to per-node data directory
        let node_dir = format!("{}/node-{}", data_dir, i);
        fs::create_dir_all(&node_dir)?;
        let p2p_key_path = Path::new(&node_dir).join("p2p_keypair");
        if let Ok(ed25519_kp) = p2p_keypairs[i].clone().try_into_ed25519() {
            fs::write(&p2p_key_path, ed25519_kp.secret().as_ref())?;
        }

        // Save keystore
        let pk_bytes = keys.signing_keys.private_key_bytes();
        let pub_key_bytes = keys.signing_keys.public_key_bytes();
        let keystore = Keystore::new(&pk_bytes, "dev-password", &pub_key_bytes)?;
        let keystore_path = format!("{}/keystore.json", node_dir);
        keystore.save_to_path(&keystore_path)?;

        // Save VRF private key
        let vrf_priv_path = format!("{}/vrf_private_key", node_dir);
        fs::write(&vrf_priv_path, hex::encode(keys.vrf_keys.secret.to_bytes()))?;
    }
    println!("\n══════════════════════════════════════════════════════════════");
    info!("[BOOTSTRAP] genesis.json and node data written to ./{}/", data_dir);
    info!("[BOOTSTRAP] Default keystore password: \"dev-password\"");

    Ok(())
}

// Normal Node Runner
async fn run_node(args: Args) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Load genesis
    let genesis = Genesis::from_file(&args.genesis)
        .expect("genesis.json not found or invalid.");

    info!(
        "[MAIN] Loaded genesis: chain_id={}, validators={}, committee_size={}",
        genesis.chain_id,
        genesis.accounts.len(),
        genesis.parameters.sub_committee_size
    );

    // 2. Load or generate P2P keypair
    let p2p_keypair = load_or_create_p2p_keypair(&args.data_dir)?;
    let local_peer_id = PeerId::from(p2p_keypair.public());
    info!("[MAIN] Local Peer ID: {}", local_peer_id);

    // 3. Load validator keys (signing + VRF) from keystore
    let validator_keys = load_validator_keys(&args)?;
    let my_address = public_key_to_address(&validator_keys.signing_keys.public_key_bytes());
    info!(
        "[MAIN] Running as validator: 0x{}",
        hex::encode(my_address.as_ref())
    );

    // 4. Initialize AddressBook from genesis
    let address_book = Arc::new(Mutex::new(AddressBook::default()));
    {
        let mut ab = address_book.lock().await;
        ab.update_from_genesis(&genesis);
        info!(
            "[MAIN] AddressBook initialized with {} peers from genesis.",
            ab.get_all_peer_ids().len()
        );
    }

    // 5. Create all channels
    let (p2p_cmd_tx, p2p_cmd_rx) = mpsc::channel::<P2pCommand>(100);
    let (consensus_msg_tx, consensus_msg_rx) = mpsc::channel(100);
    let (tx_gossip, _rx_gossip) = mpsc::channel::<ChainMessage>(100);
    let (txs_response_tx, txs_response_rx) = mpsc::channel::<SyncResponse>(100);
    let (confirmed_batch_tx, mut confirmed_batch_rx) = mpsc::channel::<PayloadBatch>(64);

    // 6. Initialize consensus state
    let initial_qc = QuorumCertificate::genesis_qc();
    let state = ConsensusState::new(initial_qc);
    let consensus_state_for_p2p = Some(Arc::new(RwLock::new(state.clone())));

    let p2p_ready_flag = Arc::new(AtomicBool::new(false));
    let consensus_ready_flag = p2p_ready_flag.clone(); 
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let mempool = Arc::new(RwLock::new(Vec::<AppPayload>::new()));

    // 7. Build and spawn ConsensusEngine
    let engine = ConsensusEngine {
        my_address,
        validator_keys: Arc::new(validator_keys),
        p2p_cmd_tx: p2p_cmd_tx.clone(),
        state: state.clone(),
        consensus_ready: consensus_ready_flag.clone(),
        address_book: Arc::clone(&address_book),
        pending_tx_requests: Arc::new(RwLock::new(HashMap::new())),
        tx_gossip: tx_gossip.clone(),
        mempool: mempool.clone(),
        chain_id: genesis.chain_id.clone(),
        genesis_params: genesis.parameters.clone(),
        confirmed_batch_tx,
        shutdown: shutdown_flag.clone(),
    };

    let engine_for_shutdown = engine.clone();
    tokio::spawn(engine.run(consensus_msg_rx, txs_response_rx));
    info!("[MAIN] Consensus engine spawned.");

    // 8. Spawn P2P layer
    let is_bootstrap_node = args.bootstrap_node.is_empty();
    let bootstrap_nodes = args.bootstrap_node.clone();

    if !bootstrap_nodes.is_empty() {
        let delay_ms = rand::rng().random_range(500u64..2000);
        info!(
            "[MAIN] Non-bootstrap node, waiting {}ms before starting P2P...",
            delay_ms
        );
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    let p2p_handle = tokio::spawn(p2p::run(
        p2p_keypair,
        bootstrap_nodes,
        args.p2p_port,
        consensus_msg_tx,
        txs_response_tx,
        is_bootstrap_node,
        consensus_state_for_p2p,
        p2p_cmd_rx,
        p2p_cmd_tx.clone(),
        p2p_ready_flag.clone(),
        Arc::clone(&address_book),
    ));
    info!("[MAIN] P2P layer spawned on port {}.", args.p2p_port);

    // 9. Spawn confirmed batch listener
    let _batch_listener_handle = tokio::spawn(async move {
        info!("[BATCH LISTENER] Listening for confirmed batches...");
        while let Some(batch) = confirmed_batch_rx.recv().await {
            info!(
                "╔══════════════════════════════════════════════════════════╗"
            );
            info!(
                "║  BATCH #{} CONFIRMED                                    ",
                batch.header.index
            );
            info!(
                "║  Round: {} | Payloads: {} | Hash: 0x{}",
                batch.round,
                batch.payloads.len(),
                hex::encode(&batch.header.calculate_hash()[..8])
            );
            info!(
                "╚══════════════════════════════════════════════════════════╝"
            );
        }
        info!("[BATCH LISTENER] Channel closed. Stopping.");
    });

    // 10. Wait for shutdown signal (Ctrl+C)
    info!("[MAIN] Node is running. Press Ctrl+C to shut down.");
    tokio::select! {
        res = p2p_handle => {
            match res {
                Ok(Ok(())) => info!("[MAIN] P2P task exited cleanly."),
                Ok(Err(e)) => error!("[MAIN] P2P task exited with error: {}", e),
                Err(e) => error!("[MAIN] P2P task panicked: {}", e),
            }
        },
        _ = tokio::signal::ctrl_c() => {
            info!("[MAIN] Ctrl+C received. Shutting down gracefully...");
            engine_for_shutdown.request_shutdown();
        },
    }

    // Give tasks a moment to clean up
    tokio::time::sleep(Duration::from_millis(500)).await;
    info!("[MAIN] Node shut down complete.");
    Ok(())
}

// Utility: P2P Keypair
fn load_or_create_p2p_keypair(
    data_dir: &str,
) -> Result<P2pKeypair, Box<dyn std::error::Error + Send + Sync>> {
    let p2p_key_path = Path::new(data_dir).join("p2p_keypair");

    if p2p_key_path.exists() {
        let mut key_bytes = fs::read(&p2p_key_path)?;
        let secret_key = ed25519::SecretKey::try_from_bytes(&mut key_bytes)
            .map_err(|e| format!("Corrupt P2P keypair file: {}", e))?;
        let keypair = P2pKeypair::from(ed25519::Keypair::from(secret_key));
        info!("[P2P] Loaded existing keypair from {:?}", p2p_key_path);
        Ok(keypair)
    } else {
        let ed25519_keypair = ed25519::Keypair::generate();
        fs::create_dir_all(data_dir)?;
        fs::write(&p2p_key_path, ed25519_keypair.secret().as_ref())?;
        info!("[P2P] Generated and saved new keypair to {:?}", p2p_key_path);
        Ok(P2pKeypair::from(ed25519_keypair))
    }
}

// Utility: Validator Key Loading
fn load_validator_keys(args: &Args) -> Result<ValidatorKeys, Box<dyn std::error::Error + Send + Sync>> {
    let keystore_path = args.keystore_path.as_ref()
        .expect("--keystore-path is required to run as a validator node.");
    let vrf_priv_key_hex = args.vrf_priv_key.as_ref()
        .expect("--vrf-priv-key is required to run as a validator node.");

    info!("[KEYS] Loading keystore from: {}", keystore_path);
    let keystore = Keystore::from_path(keystore_path)?;

    let password = match &args.password {
        Some(p) => {
            info!("[KEYS] Using password from CLI/env.");
            p.clone()
        }
        None => {
            eprint!("Enter keystore password: ");
            rpassword::read_password()?
        }
    };

    let sk_bytes = keystore.decrypt(&password)?;
    let pk_bytes = hex::decode(&keystore.public_key)?;
    let signing_keys = KeyPair::from_key_bytes(&pk_bytes, &sk_bytes)?;

    let vrf_secret_bytes = hex::decode(vrf_priv_key_hex)?;
    let vrf_secret = SchnorrkelSecretKey::from_bytes(&vrf_secret_bytes)
        .map_err(|_| "Invalid VRF private key. Ensure it is a 64-byte hex string.")?;
    let vrf_keys = vrf_secret.to_keypair();

    info!("[KEYS] Validator keys loaded successfully.");
    Ok(ValidatorKeys {
        signing_keys,
        vrf_keys,
    })
}
