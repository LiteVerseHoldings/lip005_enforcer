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

## Start The Litecoin RPC Proxy

Litecoin Core returns JSON-RPC success responses with `error: null` and uses a
different REST header path shape than the enforcer client expects. Start the
proxy from `drivechain-evm` before launching the enforcer:

```powershell
cd ..\drivechain-evm
$env:LITECOIN_PROXY_PORT = "39333"
$env:LITECOIN_RPC_PORT = "39332"
node tools\litecoin-rpc-proxy.js
cd ..\bip300301_enforcer
```

## Run The Enforcer

From `bip300301_enforcer`:

```powershell
cargo run -- `
  --mainchain litecoin `
  --data-dir .\litecoin-signet-enforcer-data `
  --node-rpc-addr 127.0.0.1:39333 `
  --node-rpc-user user `
  --node-rpc-pass password `
  --node-zmq-addr-sequence tcp://127.0.0.1:39000
```

Litecoin mode supports validator sync, an experimental `--enable-wallet` path
backed by the loaded Litecoin Core wallet, and Litecoin signet
`getblocktemplate` startup. If the Litecoin Core node does not expose wallet RPC
methods, pass an explicit coinbase output script:

```powershell
cargo run -- `
  --mainchain litecoin `
  --data-dir .\litecoin-signet-enforcer-data `
  --node-rpc-addr 127.0.0.1:39333 `
  --node-rpc-user user `
  --node-rpc-pass password `
  --node-zmq-addr-sequence tcp://127.0.0.1:39000 `
  --enable-wallet `
  --wallet-sync-source disabled `
  --enable-mempool `
  --signet-miner-coinbase-script-pubkey 00141111111111111111111111111111111111111111
```

This means the current Litecoin path verifies block sync and validator state,
can delegate wallet funding/signing calls to Litecoin Core, can broadcast and
include deposit transactions, and can serve a block template for the controlled
signet. The controlled signet miner can use that enforcer template by pointing
`LITECOIN_GBT_URL` at `127.0.0.1:8122`:

```powershell
$env:LITECOIN_GBT_URL = "http://127.0.0.1:8122/"
$env:LITECOIN_SUBMIT_URL = "http://127.0.0.1:39332/"
$env:LITECOIN_RPC_URL = "http://127.0.0.1:39332/"
$env:LITECOIN_SIGNET_AUTHORITY_FILE = "C:\path\to\litecoin-signet-authority\authority.json"
python ..\drivechain-evm\tools\mine-controlled-litecoin-signet.py 1
```

The drivechain lifecycle script can also create a sidechain proposal and mine
through activation:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File ..\drivechain-evm\scripts\Test-DrivechainLifecycle.ps1 -ActivateSidechainProposal
```

To create, broadcast, mine, and verify a deposit CTIP after activation, add
`-AttemptDepositTransaction`. This requires a funded loaded Litecoin Core
wallet and the wallet-enabled Litecoin Core build used by
`Start-LitecoinSignet.ps1`.

The full Drivechain lifecycle is still blocked on withdrawal bundle paths and
longer-running operational hardening.

## Mine A Test Block

From `drivechain-evm`:

```powershell
$env:LITECOIN_SIGNET_AUTHORITY_FILE = "C:\path\to\litecoin-signet-authority\authority.json"
python tools\mine-controlled-litecoin-signet.py 1
```

The signet authority file is intentionally private and should not be committed.

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
