# E-Sequencer Node

Thin binary for running an [Evice Sequencer](https://github.com/evice-labs/e-sequencer) node. This binary handles CLI configuration, key management, P2P networking, and consensus lifecycle — delegating all ordering logic to the `e-sequencer` library.

## Quick Start

### 1. Bootstrap (Generate Genesis + Keys)

```bash
cargo run -- --bootstrap --num-validators 6
```

This generates:
- `genesis.json` — Network configuration with all validator identities
- `node_data/node-{0..5}/` — Per-node directories containing:
  - `p2p_keypair` — Persistent P2P identity
  - `keystore.json` — Encrypted signing keys (password: `dev-password`)
  - `vrf_private_key` — VRF secret key for leader election

### 2. Run with Docker Compose

Build the Docker images (required on first run or after code changes):
```bash
docker compose build
```

Start all 6 sequencer nodes:
```bash
docker compose up
```

Or combine both in one command:
```bash
docker compose up --build
```
- Loads its VRF private key from `{data_dir}/vrf_private_key`
- Resolves bootstrap peers from `genesis.json` (no manual Peer ID configuration required)
- Connects to other nodes via P2P and begins the BFT consensus process

To stop the network:
```bash
docker compose down
```

### 3. Run a Single Node (Manual)

If you prefer running nodes individually without Docker:

```bash
cargo run -- --dev \
  --p2p-port 9000 \
  --data-dir ./node_data/node-0 \
  --keystore-path ./node_data/node-0/keystore.json \
  --password dev-password
```

VRF key and bootstrap peers are auto-loaded from the data directory and genesis file. To override manually:

```bash
cargo run -- --dev \
  --p2p-port 9001 \
  --data-dir ./node_data/node-1 \
  --keystore-path ./node_data/node-1/keystore.json \
  --vrf-priv-key $(cat ./node_data/node-1/vrf_private_key) \
  --password dev-password \
  --bootstrap-node /ip4/127.0.0.1/tcp/9000/p2p/<NODE_0_PEER_ID>
```

## CLI Reference

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--bootstrap` | — | `false` | Generate genesis + keys and exit |
| `--num-validators` | — | `6` | Number of validators (bootstrap mode) |
| `--p2p-port` | `P2P_PORT` | `9000` | P2P listening port |
| `--genesis` | `GENESIS_PATH` | `genesis.json` | Path to genesis file |
| `--data-dir` | `DATA_DIR` | `./node_data` | Data directory |
| `--keystore-path` | `KEYSTORE_PATH` | — | Path to keystore JSON |
| `--vrf-priv-key` | `VRF_PRIV_KEY` | *(auto from data-dir)* | Hex VRF private key |
| `--password` | `KEYSTORE_PASSWORD` | *(prompted)* | Keystore password |
| `--dev` | `DEV_MODE` | `false` | Allow loopback P2P |
| `--get-peer-id` | — | — | Print PeerId and exit |
| `--bootstrap-node` | — | *(auto from genesis)* | Bootstrap node multiaddr(s) |

## License

Dual-licensed under MIT and Apache 2.0
