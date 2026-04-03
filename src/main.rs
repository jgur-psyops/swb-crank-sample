use anyhow::{Context, Result};
use futures::{StreamExt, stream::FuturesUnordered};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use solana_client::nonblocking::rpc_client::RpcClient;
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

use switchboard_on_demand::client::{
    CrossbarClient, Gateway, PullFeed,
    pull_feed::{FetchUpdateParams, SbContext},
};

// ---------- FEEDS AS CONSTANTS ----------
const FEEDS: &[&str] = &[
    "BRCWKZ1PevwTFzBL2MLZM5hMwCNMWE8NYuS6zFPaXZ6y",
    "5QModpg2kw1EwWPZHHSAHc5wozGyhYuSQ5K544tsMvF8",
    "DnNKEemtzfCg6wrCR1X5iiPyecKNTxozQW2dRQiY6GNu",
    "5htZ4vPKPjAEg8EJv6JHcaCetMM4XehZo8znQvrp6Ur3",
    "4V8VRQdfnSzfdhGB2ZcAPK4Ts32edKo83TnRmx6eMYAu",
    "EcL93EQPJUgGehECHKhQ96cs6LWJXktzf7to92d9AjCT",
    "9UivckJDKtDChtXvCqgDxGS2CmA4Z9Zb14CMZ76n1PNp",
    "Hb3u8TcuBWv2SNhUhqNyPwh6RHqTVBFrwpATcvGtsBQN",
    "4Fcc3CurbfYC4tfQmNtTEHRmpurgnGYeZyrKZtURWbLZ",
    "ELTPRwqTD99D5qhhJv6Q7NpScPMuEsXHo8eZTEwh1n5q",
    "DMhGWtLAKE5d56WdyHQxqeFncwUeqMEnuC2RvvZfbuur",
    "CEyicmAQXJKvXuRrFpfjHUs5du5g8JcZDRE5UbGcprN2",
    "2Ss1JWyej6sydX6wtNfNux4FpPNgoof4VdsBEJ1FzooS",
    "2wJdP1WM5dApaXf5DXVnPzcEqfqnwwEofjvJVvn6FNws",
    "2kn1FZYNwApofSxAUY2cBArkZ5ja3aqBRXnLuuYrZM3U",
    "Cw7biY8M5mnq346fM4ewpU9ezHAFwNnduBxRDByDbsa8",
    "EMhYJjDUJYCtNWKxoj39u8AkVsUtm6yDz8wsFfqDCfG5",
    "FxdT3KvBU4Ect6BqcJPvRRzKXDjk9PVc3ybb874k7M5j",
    "4svXjRbiJ9Rw3fJykiTm2NH5JLRza5K768q6LBmCt7Cv",
    "3xjjfYi2o2nN4w6Jhf3Cf66BgYzMtYqhXojYTgwJMCQq",
    "BWK8Wnybb7rPteNMqJs9uWoqdfYApNym6WgE59BwLe1v",
    "HBkp4ppTRv9HjAKLz7Wjo1zRUd1m1o7Nn3uaapCRfT21",
    "5EJRmV2BXzeXmEm6GyoTNA8EWh5JJcC19wz6AKAiqzdY",
    "CTn1eaLioKeZ53DyA5XRYVA9rWutxCfTzfvyezAPuSuC",
    "Fj9o7sui5XCwCPYErLCcyf1hsdqExRg84f3YaydJxFiH",
    "7MUWCEmm8HWa9WZJq5DutZm1B3ziTkLmaKQknAd2xu2m",
    "7V25v9BLbiS8wK32bgLo111HkjWPYbGdh8wh7tEtc2bG",
    "7AjwutSAhQkaTqSsMrnWPNrvX97vcAz5FfcCmLfpYaPz",
    "j8RP5LnubZh6VD6eJk1zguGpKkpYrCWvTEmtBosUadR",
    "48tBfE2bjtmEi4XKaQPc81SHQj3Srr1HN6AJJqeEiPNY",
    "8wRUjxh4uCdvQdqcWUMvBBTJa95vLuKrze7WLus5h6Gk",
    "HaF6jK16UwZZt9iFXRUqpSMWUFzhUJaU8rmVtXcokoTZ",
    "4tHjwSfrZFAxgFMVDrrkXTS6TwftAGHQwgrD3Hi9p1p2",
    "4YMdFbV8FaUv4FpE2gbZBReSFVzFKNPJZHirHwc2Psza",
    "BMv5mRgPVq5oe8sAeHsMaiq4o3MzJTmvvBe66AG9pwtr",
    "EtEbRr1fiigYs51PVaX6Ldupda4aMxz9qQE2iTBwLpZD",
    "3zCUYzDhuyZREWgngzJfUt2UzQ5SzKCUK5AdwiB2HYmU",
    "AEq1mcpesN4u9CSd8uUQbQar6qLghNQx7rnhdQhAnUyn",
    "4Hmd6PdjVA9auCoScE12iaBogfwS4ZXQ6VZoBeqanwWW",
    "CVjgZ9vHcsrYVy1zqAVKD9f5kokwsYDbGaZ9Axv6uDKx",
];

#[tokio::main]
async fn main() -> Result<()> {
    let continuous_seconds = parse_continuous_seconds()?;

    // ---------- config ----------
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let gateway_url = std::env::var("SWB_GATEWAY").unwrap_or_else(|_| {
        "https://92.222.100.182.xip.switchboard-oracles.xyz/mainnet".to_string()
    });

    let default_kp = format!(
        "{}/.keys/staging-deploy.json",
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

    match continuous_seconds {
        Some(interval_secs) => {
            println!("Continuous mode enabled: cranking every {interval_secs} seconds.");
            println!("Terminate with Ctrl+C.");

            let mut round: u64 = 1;
            loop {
                println!("\n=== round #{round} ===");
                match crank_all_chunks(&client, &ctx, &gateway, &crossbar, &payer, &feeds, 4, round)
                    .await
                {
                    Ok(()) => {}
                    Err(e) => {
                        eprintln!("round #{round} failed: {e:#}");
                    }
                }

                println!("Waiting {interval_secs}s for next round. Press Ctrl+C to stop.");
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {
                        println!("Termination signal received. Exiting cleanly.");
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {}
                }
                round += 1;
            }
        }
        None => {
            crank_all_chunks(&client, &ctx, &gateway, &crossbar, &payer, &feeds, 4, 1).await?;
        }
    }

    Ok(())
}

fn parse_continuous_seconds() -> Result<Option<u64>> {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        return Ok(None);
    };

    if first == "--continuous" || first == "-c" {
        let secs_raw = args
            .next()
            .context("Missing interval seconds. Usage: --continuous <seconds>")?;
        if args.next().is_some() {
            anyhow::bail!("Unexpected trailing args. Usage: --continuous <seconds>");
        }
        let secs = secs_raw
            .parse::<u64>()
            .with_context(|| format!("Invalid seconds value '{secs_raw}'"))?;
        if secs == 0 {
            anyhow::bail!("Continuous interval must be >= 1 second");
        }
        return Ok(Some(secs));
    }

    // Numeric shorthand: `cargo run -- 10` means continuous mode every 10 seconds.
    if args.next().is_some() {
        anyhow::bail!("Unexpected args. Usage: [--continuous <seconds>] or [<seconds>]");
    }
    let secs = first
        .parse::<u64>()
        .with_context(|| format!("Invalid arg '{first}'. Usage: [--continuous <seconds>]"))?;
    if secs == 0 {
        anyhow::bail!("Continuous interval must be >= 1 second");
    }
    Ok(Some(secs))
}

fn max_in_flight_txs() -> usize {
    std::env::var("MAX_IN_FLIGHT_TXS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(3)
}

async fn crank_all_chunks(
    client: &RpcClient,
    ctx: &Arc<SbContext>,
    gateway: &Gateway,
    crossbar: &CrossbarClient,
    payer: &Keypair,
    feeds: &[Pubkey],
    chunk_size: usize,
    round: u64,
) -> Result<()> {
    let max_in_flight = max_in_flight_txs();
    let total_chunks = feeds.len().div_ceil(chunk_size);
    println!(
        "Round #{round}: cranking {} feeds across {} tx(s) (chunk size = {})",
        feeds.len(),
        total_chunks,
        chunk_size
    );
    println!("Round #{round}: max in-flight tx jobs = {max_in_flight}");

    let chunk_batches: Vec<(usize, Vec<Pubkey>)> = feeds
        .chunks(chunk_size)
        .enumerate()
        .map(|(chunk_idx, feed_chunk)| (chunk_idx, feed_chunk.to_vec()))
        .collect();

    let mut next_batch = 0usize;
    let mut tx_jobs = FuturesUnordered::new();

    while tx_jobs.len() < max_in_flight && next_batch < chunk_batches.len() {
        let (chunk_idx, feed_list) = chunk_batches[next_batch].clone();
        let tx_label = format!("tx {}/{}", chunk_idx + 1, total_chunks);
        println!("  [{tx_label}] queued ({} feeds)", feed_list.len());
        tx_jobs.push(run_chunk_job(
            client,
            ctx,
            gateway,
            crossbar,
            payer,
            chunk_idx,
            total_chunks,
            feed_list,
        ));
        next_batch += 1;
    }

    let mut failures = 0usize;

    while let Some(result) = tx_jobs.next().await {
        match result {
            Ok((chunk_idx, feed_list, sig)) => {
                println!(
                    "  tx {}/{} ({} feeds) -> {}",
                    chunk_idx + 1,
                    total_chunks,
                    feed_list.len(),
                    sig
                );
                for feed in &feed_list {
                    println!("    • {feed}");
                }
            }
            Err(err) => {
                failures += 1;
                eprintln!("  {err:#}");
            }
        }

        while tx_jobs.len() < max_in_flight && next_batch < chunk_batches.len() {
            let (chunk_idx, feed_list) = chunk_batches[next_batch].clone();
            let tx_label = format!("tx {}/{}", chunk_idx + 1, total_chunks);
            println!("  [{tx_label}] queued ({} feeds)", feed_list.len());
            tx_jobs.push(run_chunk_job(
                client,
                ctx,
                gateway,
                crossbar,
                payer,
                chunk_idx,
                total_chunks,
                feed_list,
            ));
            next_batch += 1;
        }
    }

    if failures > 0 {
        anyhow::bail!("{} out of {} tx(s) failed", failures, total_chunks);
    }

    Ok(())
}

async fn run_chunk_job(
    client: &RpcClient,
    ctx: &Arc<SbContext>,
    gateway: &Gateway,
    crossbar: &CrossbarClient,
    payer: &Keypair,
    chunk_idx: usize,
    total_chunks: usize,
    feed_list: Vec<Pubkey>,
) -> Result<(usize, Vec<Pubkey>, solana_sdk::signature::Signature)> {
    let tx_label = format!("tx {}/{}", chunk_idx + 1, total_chunks);
    let task_start = Instant::now();
    println!("  [{tx_label}] started");
    let sig = build_and_send_chunk_tx(client, ctx, gateway, crossbar, payer, &feed_list, &tx_label)
        .await
        .with_context(|| format!("{tx_label} failed"))?;
    println!(
        "  [{tx_label}] finished in {:.2}s",
        task_start.elapsed().as_secs_f64()
    );
    Ok((chunk_idx, feed_list, sig))
}

async fn build_and_send_chunk_tx(
    client: &RpcClient,
    ctx: &Arc<SbContext>,
    gateway: &Gateway,
    crossbar: &CrossbarClient,
    payer: &Keypair,
    feed_chunk: &[Pubkey],
    tx_label: &str,
) -> Result<solana_sdk::signature::Signature> {
    let tx_start = Instant::now();
    let mut update_ixs: Vec<Instruction> = Vec::with_capacity(feed_chunk.len());
    let mut lut_map: HashMap<Pubkey, AddressLookupTableAccount> = HashMap::new();

    for (feed_idx, &feed) in feed_chunk.iter().enumerate() {
        let fetch_start = Instant::now();
        println!(
            "  [{tx_label}] fetch_update_ix {}/{} for feed {}",
            feed_idx + 1,
            feed_chunk.len(),
            feed
        );
        let (update_ix, _responses, _num_ok, luts) = tokio::time::timeout(
            Duration::from_secs(40),
            PullFeed::fetch_update_ix(
                ctx.clone(),
                client,
                FetchUpdateParams {
                    feed,
                    payer: payer.pubkey(),
                    gateway: gateway.clone(),
                    crossbar: Some(crossbar.clone()),
                    num_signatures: Some(1),
                    debug: Some(false),
                },
            ),
        )
        .await
        .with_context(|| format!("[{tx_label}] fetch_update_ix timed out for feed {feed}"))??;
        println!(
            "  [{tx_label}] fetch_update_ix done for feed {} in {:.2}s ({} LUTs)",
            feed,
            fetch_start.elapsed().as_secs_f64(),
            luts.len()
        );

        update_ixs.push(update_ix);
        for lut in luts {
            lut_map.entry(lut.key).or_insert(lut);
        }
    }

    let merged_luts: Vec<AddressLookupTableAccount> = lut_map.into_values().collect();

    let per_feed_cu: u32 = 300_000;
    let mut cu_limit = per_feed_cu.saturating_mul(feed_chunk.len() as u32);
    if cu_limit < 300_000 {
        cu_limit = 300_000;
    }
    if cu_limit > 1_400_000 {
        cu_limit = 1_400_000;
    }

    let update_ix_count = update_ixs.len();
    let mut ixs: Vec<Instruction> = Vec::with_capacity(2 + update_ix_count);
    ixs.push(ComputeBudgetInstruction::set_compute_unit_limit(cu_limit));
    ixs.push(ComputeBudgetInstruction::set_compute_unit_price(5_000));
    ixs.extend(update_ixs);

    println!(
        "  [{tx_label}] built {} update ix(s), unique LUTs={}",
        update_ix_count,
        merged_luts.len()
    );

    let blockhash_start = Instant::now();
    println!("  [{tx_label}] get_latest_blockhash");
    let latest_blockhash =
        tokio::time::timeout(Duration::from_secs(20), client.get_latest_blockhash())
            .await
            .with_context(|| format!("[{tx_label}] get_latest_blockhash timed out"))??;
    println!(
        "  [{tx_label}] got blockhash in {:.2}s",
        blockhash_start.elapsed().as_secs_f64()
    );

    println!("  [{tx_label}] compile/sign tx");
    let v0_msg = v0::Message::try_compile(&payer.pubkey(), &ixs, &merged_luts, latest_blockhash)?;
    let vtx = VersionedTransaction::try_new(VersionedMessage::V0(v0_msg), &[payer])?;

    let send_start = Instant::now();
    println!("  [{tx_label}] send_transaction");
    let sig = tokio::time::timeout(Duration::from_secs(30), client.send_transaction(&vtx))
        .await
        .with_context(|| format!("[{tx_label}] send_transaction timed out"))??;
    println!(
        "  [{tx_label}] send_transaction submitted in {:.2}s (total {:.2}s)",
        send_start.elapsed().as_secs_f64(),
        tx_start.elapsed().as_secs_f64()
    );
    Ok(sig)
}
