use anyhow::Result;
use std::collections::HashMap;
use std::str::FromStr;

use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::{VersionedMessage, v0},
    pubkey::Pubkey,
    signature::{Keypair, Signer, read_keypair_file},
    transaction::VersionedTransaction,
};

use switchboard_on_demand_client::{
    crossbar::CrossbarClient,
    gateway::Gateway,
    pull_feed::{FetchUpdateParams, PullFeed, SbContext},
};

// ---------- FEEDS AS CONSTANTS ----------
const FEEDS: &[&str] = &[
    // "4Hmd6PdjVA9auCoScE12iaBogfwS4ZXQ6VZoBeqanwWW", // SOL
    // "BWK8Wnybb7rPteNMqJs9uWoqdfYApNym6WgE59BwLe1v", // LST
    // "5htZ4vPKPjAEg8EJv6JHcaCetMM4XehZo8znQvrp6Ur3", // JITOSOL
    // "7YDhgtpNLenb4dSf77guacM7diov2obqzyLz4NNYbjWg", // TNSR
    // "6aY5Qx4k22Kws22zmTEoiEesx7XzfxHsAs26ArgXed9D", // PRCL
    // "4VmpF3ndsZiXn89PMcg7S9LcuXHsPY4n1XC7fvgrJTva", // MOBILE

    // random garbage
    "A9RnpLxxtAS2TR3HtSMNJfsKpRPvkLbBkGZ6gKziSPLr",
    "BWK8Wnybb7rPteNMqJs9uWoqdfYApNym6WgE59BwLe1v",
    "4Hmd6PdjVA9auCoScE12iaBogfwS4ZXQ6VZoBeqanwWW",
    "5htZ4vPKPjAEg8EJv6JHcaCetMM4XehZo8znQvrp6Ur3",
    "DMhGWtLAKE5d56WdyHQxqeFncwUeqMEnuC2RvvZfbuur",
];

#[tokio::main]
async fn main() -> Result<()> {
    // ---------- config ----------
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let gateway_url = std::env::var("SWB_GATEWAY").unwrap_or_else(|_| {
        "https://92.222.100.182.xip.switchboard-oracles.xyz/mainnet".to_string()
    });

    let default_kp = format!(
        "{}/keys/staging-deploy.json",
        std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
    );
    let keypair_path = std::env::var("KEYPAIR").unwrap_or(default_kp);
    let payer: Keypair = read_keypair_file(&keypair_path)
        .map_err(|e| anyhow::anyhow!("read_keypair_file({}): {e}", keypair_path))?;

    // ---------- parse feeds ----------
    let feeds: Vec<Pubkey> = FEEDS
        .iter()
        .map(|s| Pubkey::from_str(s))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Invalid feed pubkey: {e}"))?;

    if feeds.is_empty() {
        anyhow::bail!("No feeds provided in FEEDS slice");
    }

    // ---------- shared clients ----------
    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let ctx = SbContext::new();
    let gateway = Gateway::new(gateway_url);
    let crossbar = CrossbarClient::default();

    // ---------- build all update ixs & merge LUTs ----------
    // NOTE: This is effectively the "fetchUpdateManyIx" behavior: we fetch per-feed update ixs
    // and send them together in ONE transaction.
    let mut all_update_ixs: Vec<Instruction> = Vec::with_capacity(feeds.len());
    let mut lut_map: HashMap<Pubkey, AddressLookupTableAccount> = HashMap::new();

    for &feed in &feeds {
        println!("Preparing update ix for feed: {feed}");
        let (update_ix, _responses, _num_ok, luts) = PullFeed::fetch_update_ix(
            ctx.clone(),
            &client,
            FetchUpdateParams {
                feed,
                payer: payer.pubkey(),
                gateway: gateway.clone(),
                crossbar: Some(crossbar.clone()),
                num_signatures: Some(1), // tune as you wish
                debug: Some(false),
            },
        )
        .await?;

        all_update_ixs.push(update_ix);

        // merge LUTs by key to avoid duplicates
        for lut in luts {
            lut_map.entry(lut.key).or_insert(lut);
        }
    }

    let merged_luts: Vec<AddressLookupTableAccount> = lut_map.into_values().collect();

    // ---------- compute budget (single tx for many feeds) ----------
    // Max per-tx CU is 1.4M; target ~300k per feed (rough heuristic).
    let per_feed_cu: u32 = 300_000;
    let mut cu_limit = per_feed_cu.saturating_mul(feeds.len() as u32);
    if cu_limit < 300_000 {
        cu_limit = 300_000;
    }
    if cu_limit > 1_400_000 {
        cu_limit = 1_400_000;
    }

    let compute_ixes = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(cu_limit),
        // adjust price to your preference / network conditions
        ComputeBudgetInstruction::set_compute_unit_price(5_000),
    ];

    let latest_blockhash = client.get_latest_blockhash().await?;

    let mut ixs: Vec<Instruction> = Vec::with_capacity(2 + all_update_ixs.len());
    ixs.extend(compute_ixes);
    ixs.extend(all_update_ixs);

    // ---------- compile, simulate, send ----------
    let v0_msg = v0::Message::try_compile(&payer.pubkey(), &ixs, &merged_luts, latest_blockhash)?;
    let vtx = VersionedTransaction::try_new(VersionedMessage::V0(v0_msg), &[&payer])?;

    let sim = client
        .simulate_transaction_with_config(
            &vtx,
            RpcSimulateTransactionConfig {
                sig_verify: true,
                replace_recent_blockhash: false,
                commitment: Some(CommitmentConfig::processed()),
                encoding: None,
                accounts: None,
                min_context_slot: None,
                inner_instructions: true,
            },
        )
        .await?;

    if let Some(logs) = sim.value.logs.clone() {
        println!("--- simulation logs ({} feeds) ---", FEEDS.len());
        for l in logs {
            println!("{l}");
        }
        println!("----------------------------------");
    }
    if let Some(err) = sim.value.err.clone() {
        anyhow::bail!("simulation failed: {err:?}");
    }

    let sig = client.send_and_confirm_transaction(&vtx).await?;
    println!("✅ Cranked {} feeds in one tx -> {sig}", FEEDS.len());
    for f in FEEDS {
        println!("  • {f}");
    }

    Ok(())
}

// Minimal example w/ one feed
// const FEED_PUBKEY: &str = "5htZ4vPKPjAEg8EJv6JHcaCetMM4XehZo8znQvrp6Ur3";

// #[tokio::main]
// async fn main() -> Result<()> {
//     let rpc_url = std::env::var("RPC_URL")
//         .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
//     let gateway_url = std::env::var("SWB_GATEWAY").unwrap_or_else(|_| {
//         "https://92.222.100.182.xip.switchboard-oracles.xyz/mainnet".to_string()
//     });

//     let default_kp = format!(
//         "{}/keys/staging-deploy.json",
//         std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
//     );
//     let keypair_path = std::env::var("KEYPAIR").unwrap_or(default_kp);
//     let payer: Keypair = read_keypair_file(&keypair_path)
//         .map_err(|e| anyhow::anyhow!("read_keypair_file({}): {e}", keypair_path))?;

//     let feed = Pubkey::from_str(FEED_PUBKEY)?;

//     let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
//     let ctx = SbContext::new();
//     let gateway = Gateway::new(gateway_url);
//     let crossbar = CrossbarClient::default();

//     let (update_ix, _responses, _num_ok, luts) = PullFeed::fetch_update_ix(
//         ctx.clone(),
//         &client,
//         FetchUpdateParams {
//             feed,
//             payer: payer.pubkey(),
//             gateway: gateway.clone(),
//             crossbar: Some(crossbar),
//             num_signatures: Some(1),
//             debug: Some(false),
//         },
//     )
//     .await?;

//     let latest_blockhash = client.get_latest_blockhash().await?;
//     let compute_ixes = vec![
//         ComputeBudgetInstruction::set_compute_unit_limit(1_200_000),
//         ComputeBudgetInstruction::set_compute_unit_price(5_000),
//     ];

//     let mut ixs: Vec<Instruction> = compute_ixes;
//     ixs.push(update_ix);

//     let v0_msg = v0::Message::try_compile(&payer.pubkey(), &ixs, &luts, latest_blockhash)?;

//     let vtx = VersionedTransaction::try_new(VersionedMessage::V0(v0_msg), &[&payer])?;
//     let sim = client
//         .simulate_transaction_with_config(
//             &vtx,
//             RpcSimulateTransactionConfig {
//                 sig_verify: true,
//                 replace_recent_blockhash: false,
//                 commitment: Some(CommitmentConfig::processed()),
//                 encoding: None,
//                 accounts: None,
//                 min_context_slot: None,
//                 inner_instructions: true,
//             },
//         )
//         .await?;
//     if let Some(logs) = sim.value.logs.clone() {
//         println!("--- simulation logs ---");
//         for l in logs {
//             println!("{l}");
//         }
//         println!("-----------------------");
//     }
//     if let Some(err) = sim.value.err.clone() {
//         anyhow::bail!("simulation failed: {err:?}");
//     }
//     // END INSERT —>

//     let sig = client.send_and_confirm_transaction(&vtx).await?;
//     println!("✅ Cranked {feed} -> {sig}");
//     Ok(())
// }
