use anyhow::Result;
use std::str::FromStr;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
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

const FEED_PUBKEY: &str = "4Hmd6PdjVA9auCoScE12iaBogfwS4ZXQ6VZoBeqanwWW";

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
            num_signatures: Some(8),
            debug: Some(false),
        },
    )
    .await?;

    let latest_blockhash = client.get_latest_blockhash().await?;
    let ixs: Vec<Instruction> = vec![update_ix];

    let v0_msg = v0::Message::try_compile(&payer.pubkey(), &ixs, &luts, latest_blockhash)?;

    let vtx = VersionedTransaction::try_new(VersionedMessage::V0(v0_msg), &[&payer])?;
    let sig = client.send_and_confirm_transaction(&vtx).await?;
    println!("✅ Cranked {feed} -> {sig}");

    Ok(())
}
