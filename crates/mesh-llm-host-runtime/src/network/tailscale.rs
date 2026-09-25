use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;

const DEFAULT_MESH_API_PORT: u16 = 9337;
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct TailscaleMeshPeer {
    pub hostname: String,
    pub address: String,
    pub os: Option<String>,
    pub models: Vec<String>,
    pub api_base_url: String,
    /// Used internally by auto-join only; never expose the bootstrap token in
    /// the general discovery response.
    #[serde(skip_serializing)]
    pub invite_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TailscaleStatus {
    #[serde(rename = "Self")]
    self_peer: TailscalePeer,
    #[serde(rename = "Peer", default)]
    peers: HashMap<String, TailscalePeer>,
}

#[derive(Debug, Deserialize)]
struct TailscalePeer {
    #[serde(rename = "HostName", default)]
    hostname: String,
    #[serde(rename = "DNSName", default)]
    dns_name: String,
    #[serde(rename = "TailscaleIPs", default)]
    addresses: Vec<String>,
    #[serde(rename = "OS")]
    os: Option<String>,
    #[serde(rename = "Online", default)]
    online: bool,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct TailscaleJoinResponse {
    invite_token: String,
}

pub(crate) fn is_tailscale_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]),
        std::net::IpAddr::V6(ip) => {
            let segments = ip.segments();
            segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
        }
    }
}

/// Return whether an address is currently assigned to a peer known by the local
/// Tailscale control plane. This is deliberately stronger than checking whether
/// an address falls inside Tailscale's IPv4 CGNAT range: a CGNAT address alone is
/// not proof that the TCP caller is a tailnet member.
pub(crate) fn is_known_tailscale_peer(ip: std::net::IpAddr) -> Result<bool> {
    let status = read_status()?;
    let matches = |peer: &TailscalePeer| {
        peer.addresses.iter()
            .filter_map(|address| address.parse::<std::net::IpAddr>().ok())
            .any(|address| address == ip)
    };
    Ok(matches(&status.self_peer) || status.peers.values().any(matches))
}

pub async fn discover_mesh_peers(
    target_name: Option<&str>,
    timeout: Duration,
) -> Result<Vec<TailscaleMeshPeer>> {
    let status = read_status()?;
    let client = Client::builder()
        .timeout(timeout.max(DEFAULT_PROBE_TIMEOUT))
        .build()
        .context("build Tailscale discovery HTTP client")?;

    let self_addresses = status.self_peer.addresses;
    let mut candidates = Vec::new();

    for peer in status.peers.into_values() {
        if !peer.online || peer.addresses.iter().any(|addr| self_addresses.contains(addr)) {
            continue;
        }

        let hostname = if !peer.hostname.trim().is_empty() {
            peer.hostname.trim().to_string()
        } else {
            peer.dns_name.trim_end_matches('.').to_string()
        };

        if let Some(target) = target_name {
            if !hostname.eq_ignore_ascii_case(target)
                && !peer.dns_name.eq_ignore_ascii_case(target)
            {
                continue;
            }
        }

        let Some(address) = peer
            .addresses
            .iter()
            .find(|addr| addr.parse::<std::net::Ipv4Addr>().is_ok())
        else {
            continue;
        };

        let api_base_url = format!("http://{address}:{DEFAULT_MESH_API_PORT}");
        let response = match client
            .get(format!("{api_base_url}/v1/models"))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => response,
            _ => continue,
        };

        let models = match response.json::<ModelsResponse>().await {
            Ok(body) => body.data.into_iter().map(|model| model.id).collect(),
            Err(_) => continue,
        };

        let invite_token = match client
            .get(format!("{api_base_url}/api/tailscale/join"))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => response
                .json::<TailscaleJoinResponse>()
                .await
                .ok()
                .map(|body| body.invite_token),
            _ => None,
        };

        candidates.push(TailscaleMeshPeer {
            hostname,
            address: address.clone(),
            os: peer.os,
            models,
            api_base_url,
            invite_token,
        });
    }

    candidates.sort_by(|a, b| a.hostname.cmp(&b.hostname));
    Ok(candidates)
}

fn read_status() -> Result<TailscaleStatus> {
    let output = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .context("run tailscale status --json; is Tailscale installed and running?")?;

    if !output.status.success() {
        bail!(
            "tailscale status --json failed with exit status {}",
            output.status
        );
    }

    serde_json::from_slice(&output.stdout).context("decode Tailscale status JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_response_decodes() {
        let response: ModelsResponse =
            serde_json::from_str(r#"{"data":[{"id":"Qwen3-8B"},{"id":"Llama-3"}]}"#)
                .expect("models response should decode");

        assert_eq!(response.data[0].id, "Qwen3-8B");
        assert_eq!(response.data[1].id, "Llama-3");
    }

    #[test]
    fn discovery_peer_serialization_does_not_expose_invite_token() {
        let peer = TailscaleMeshPeer {
            hostname: "worker".to_string(),
            address: "100.64.0.2".to_string(),
            os: Some("linux".to_string()),
            models: vec!["Qwen3-8B".to_string()],
            api_base_url: "http://100.64.0.2:9337".to_string(),
            invite_token: Some("secret-invite-token".to_string()),
        };
        let json = serde_json::to_string(&peer).expect("peer should serialize");
        assert!(!json.contains("secret-invite-token"));
        assert!(!json.contains("invite_token"));
    }

    #[test]
    fn tailscale_status_decodes_peer_map() {
        let status: TailscaleStatus = serde_json::from_str(
            r#"{
                "Self":{"HostName":"router","TailscaleIPs":["100.64.0.1"],"Online":true},
                "Peer":{
                    "node-key":{"HostName":"worker","TailscaleIPs":["100.64.0.2"],"OS":"linux","Online":true}
                }
            }"#,
        )
        .expect("status should decode");

        assert_eq!(status.self_peer.hostname, "router");
        assert_eq!(status.peers["node-key"].hostname, "worker");
        assert!(status.peers["node-key"].online);
    }
}
