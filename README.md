# E-Sequencer Node

Thin binary for running an [Evice Sequencer](https://github.com/evice-labs/e-sequencer) node. This binary handles CLI configuration, key management, P2P networking, and consensus lifecycle — delegating all ordering logic to the `e-sequencer` library.

## Quick Start

### 1. Bootstrap (Generate Genesis + Keys)

```bash
cargo run -- --bootstrap --num-validators 4
```

This generates:
- `genesis.json` — Network configuration with all validator identities
- `node_data/node-{0..3}/` — Per-node directories containing:
  - `p2p_keypair` — Persistent P2P identity
  - `keystore.json` — Encrypted signing keys (password: `dev-password`)
  - `vrf_private_key` — VRF secret key

### 2. Run a Node

```bash
# Node 0 (bootstrap node — no --bootstrap-node flag)
cargo run -- --dev \
  --p2p-port 9000 \
  --data-dir ./node_data/node-0 \
  --keystore-path ./node_data/node-0/keystore.json \
  --vrf-priv-key $(cat ./node_data/node-0/vrf_private_key) \
  --password dev-password

# Node 1 (connects to node 0)
cargo run -- --dev \
  --p2p-port 9001 \
  --data-dir ./node_data/node-1 \
  --keystore-path ./node_data/node-1/keystore.json \
  --vrf-priv-key $(cat ./node_data/node-1/vrf_private_key) \
  --password dev-password \
  --bootstrap-node /ip4/127.0.0.1/tcp/9000/p2p/<NODE_0_PEER_ID>
```

### 3. Docker Compose

```bash
# After running bootstrap, update docker-compose.yml with the correct Peer IDs, then:
docker compose up
```

## CLI Reference

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--bootstrap` | — | `false` | Generate genesis + keys and exit |
| `--num-validators` | — | `4` | Number of validators (bootstrap mode) |
| `--p2p-port` | `P2P_PORT` | `9000` | P2P listening port |
| `--genesis` | `GENESIS_PATH` | `genesis.json` | Path to genesis file |
| `--data-dir` | `DATA_DIR` | `./node_data` | Data directory |
| `--keystore-path` | `KEYSTORE_PATH` | — | Path to keystore JSON |
| `--vrf-priv-key` | `VRF_PRIV_KEY` | — | Hex VRF private key |
| `--password` | `KEYSTORE_PASSWORD` | *(prompted)* | Keystore password |
| `--dev` | `DEV_MODE` | `false` | Allow loopback P2P |
| `--get-peer-id` | — | — | Print PeerId and exit |
| `--bootstrap-node` | — | — | Bootstrap node multiaddr(s) |

## License

Dual-licensed under MIT and Apache 2.0
