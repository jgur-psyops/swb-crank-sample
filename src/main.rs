use anyhow::Result;
use std::str::FromStr;

use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSimulateTransactionConfig};
use solana_sdk::{
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

// const FEED_PUBKEY: &str = "4Hmd6PdjVA9auCoScE12iaBogfwS4ZXQ6VZoBeqanwWW"; // SOL
// const FEED_PUBKEY: &str = "BWK8Wnybb7rPteNMqJs9uWoqdfYApNym6WgE59BwLe1v"; // LST
const FEED_PUBKEY: &str = "5htZ4vPKPjAEg8EJv6JHcaCetMM4XehZo8znQvrp6Ur3"; // JITOSOL
// const FEED_PUBKEY: &str = "7YDhgtpNLenb4dSf77guacM7diov2obqzyLz4NNYbjWg"; // TNSR
// const FEED_PUBKEY: &str = "6aY5Qx4k22Kws22zmTEoiEesx7XzfxHsAs26ArgXed9D"; // PRCL
// const FEED_PUBKEY: &str = "4VmpF3ndsZiXn89PMcg7S9LcuXHsPY4n1XC7fvgrJTva"; // MOBILE


#[tokio::main]
async fn main() -> Result<()> {
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

    let feed = Pubkey::from_str(FEED_PUBKEY)?;

    let client = RpcClient::new_with_commitment(rpc_url, CommitmentConfig::confirmed());
    let ctx = SbContext::new();
    let gateway = Gateway::new(gateway_url);
    let crossbar = CrossbarClient::default();

    let (update_ix, _responses, _num_ok, luts) = PullFeed::fetch_update_ix(
        ctx.clone(),
        &client,
        FetchUpdateParams {
            feed,
            payer: payer.pubkey(),
            gateway: gateway.clone(),
            crossbar: Some(crossbar),
            num_signatures: Some(1),
            debug: Some(false),
        },
    )
    .await?;

    let latest_blockhash = client.get_latest_blockhash().await?;
    let compute_ixes = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(1_200_000),
        ComputeBudgetInstruction::set_compute_unit_price(5_000),
    ];

    let mut ixs: Vec<Instruction> = compute_ixes;
    ixs.push(update_ix);

    let v0_msg = v0::Message::try_compile(&payer.pubkey(), &ixs, &luts, latest_blockhash)?;

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
        println!("--- simulation logs ---");
        for l in logs {
            println!("{l}");
        }
        println!("-----------------------");
    }
    if let Some(err) = sim.value.err.clone() {
        anyhow::bail!("simulation failed: {err:?}");
    }
    // END INSERT —>

    let sig = client.send_and_confirm_transaction(&vtx).await?;
    println!("✅ Cranked {feed} -> {sig}");
    Ok(())
}
