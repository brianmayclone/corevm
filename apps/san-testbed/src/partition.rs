//! Network partition simulation via peer address manipulation.

use reqwest::Client;
use std::collections::HashMap;

/// Store original peer addresses so we can restore them on heal.
pub type OriginalAddresses = HashMap<(usize, String), String>;

/// Apply a network partition: nodes in group_a cannot reach group_b and vice versa.
/// Uses POST /api/peers/join to update addresses to an unreachable endpoint.
pub async fn apply_partition(
    client: &Client,
    nodes: &[(usize, u16, String)], // (index, port, node_id)
    group_a: &[usize],
    group_b: &[usize],
    peer_secret: &str,
    original: &mut OriginalAddresses,
) -> Result<(), String> {
    let invalid_addr = "http://127.0.0.1:1";

    // For each node in group_a, set peers in group_b to invalid address
    for &a_idx in group_a {
        let a_port = nodes.iter().find(|n| n.0 == a_idx).unwrap().1;
        for &b_idx in group_b {
            let b = nodes.iter().find(|n| n.0 == b_idx).unwrap();
            let b_node_id = &b.2;
            let b_real_addr = format!("http://127.0.0.1:{}", b.1);

            original.entry((a_idx, b_node_id.clone()))
                .or_insert(b_real_addr);

            update_peer_address(client, a_port, b_node_id, invalid_addr, peer_secret).await?;
        }
    }

    // For each node in group_b, set peers in group_a to invalid address
    for &b_idx in group_b {
        let b_port = nodes.iter().find(|n| n.0 == b_idx).unwrap().1;
        for &a_idx in group_a {
            let a = nodes.iter().find(|n| n.0 == a_idx).unwrap();
            let a_node_id = &a.2;
            let a_real_addr = format!("http://127.0.0.1:{}", a.1);

            original.entry((b_idx, a_node_id.clone()))
                .or_insert(a_real_addr);

            update_peer_address(client, b_port, a_node_id, invalid_addr, peer_secret).await?;
        }
    }

    Ok(())
}

/// Heal all partitions — restore original peer addresses.
pub async fn heal_all(
    client: &Client,
    nodes: &[(usize, u16, String)],
    original: &mut OriginalAddresses,
    peer_secret: &str,
) -> Result<(), String> {
    for ((node_idx, peer_id), real_addr) in original.drain() {
        let port = nodes.iter().find(|n| n.0 == node_idx).unwrap().1;
        update_peer_address(client, port, &peer_id, &real_addr, peer_secret).await?;
    }
    Ok(())
}

async fn update_peer_address(
    client: &Client,
    node_port: u16,
    peer_node_id: &str,
    new_address: &str,
    peer_secret: &str,
) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{}/api/peers/join", node_port);
    client.post(&url)
        .header("X-Peer-Secret", peer_secret)
        .json(&serde_json::json!({
            "node_id": peer_node_id,
            "address": new_address,
            "hostname": format!("testbed-{}", peer_node_id),
            "peer_port": 7544,
            "secret": peer_secret,
        }))
        .send().await
        .map_err(|e| format!("partition update failed for port {}: {}", node_port, e))?;
    Ok(())
}
