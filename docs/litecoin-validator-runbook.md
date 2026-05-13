# Litecoin Validator Runbook

This runbook covers the local LiteVerse stack used to validate Litecoin mode:

- `drivechain-evm` for Besu, Litecoin signet scripts, and health checks
- `litecoin` for the patched Litecoin Core build
- `bip300301_enforcer` for Litecoin validator mode

The examples assume all three repositories are checked out under the same
workspace directory.

## Prerequisites

- Docker Desktop
- PowerShell
- Rust and Cargo 1.88.0 or newer
- A built Litecoin fork at `..\litecoin\src\litecoind`

## Start Besu

From `drivechain-evm`:

```powershell
cd infra\besu
docker compose up -d
cd ..\..
```

Check the EVM chain:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\Check-Chain.ps1 -RpcUrl http://127.0.0.1:8545
```

Expected result: four validators are running, peer count is at least `3`, and
block height advances.

## Start Litecoin Signet

From `drivechain-evm`:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\Start-LitecoinSignet.ps1 -Replace
```

Default local ports:

| Service | Address |
| --- | --- |
| Litecoin RPC | `127.0.0.1:39332` |
| Litecoin P2P | `127.0.0.1:39335` |
| Litecoin ZMQ sequence | `tcp://127.0.0.1:39000` |

Check the Litecoin node:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\Check-LitecoinSignet.ps1
```

Expected result: chain is `signet`, RPC responds, and the best block is shown.

## Run The Enforcer

From `bip300301_enforcer`:

```powershell
cargo run -- `
  --mainchain litecoin `
  --data-dir .\litecoin-signet-enforcer-data `
  --node-rpc-addr 127.0.0.1:39332 `
  --node-rpc-user user `
  --node-rpc-pass password `
  --node-zmq-addr-sequence tcp://127.0.0.1:39000
```

Litecoin mode is validator-only. Do not pass wallet, mempool, or block-file
options.

## Mine A Test Block

From `drivechain-evm`:

```powershell
python tools\mine-controlled-litecoin-signet.py 1
```

Then re-run:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\Check-LitecoinSignet.ps1
```

Expected result: Litecoin signet height increases and the enforcer remains
reachable on its JSON-RPC endpoint.

## Test Before Pushing

From `bip300301_enforcer`:

```powershell
cargo test
```

From `drivechain-evm\faucet-worker`:

```powershell
npm run check
```

From `drivechain-evm`:

```powershell
python -m py_compile tools\mine-controlled-litecoin-signet.py tools\litecoin_scrypt.py
node --check tools\litecoin-rpc-proxy.js
```
