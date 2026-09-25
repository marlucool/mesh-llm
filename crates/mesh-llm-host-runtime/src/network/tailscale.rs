// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, bail};
use futures_util::stream::{self, StreamExt};
use reqwest::Client;
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::net::IpAddr;
use std::process::Command;
use std::time::{Duration, Instant};

const DEFAULT_MESH_API_PORT: u16 = 9337;
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const TAILSCALE_PROBE_CONCURRENCY: usize = 16;
const REQUIRED_MESHLLM_TAILSCALE_TAG: &str = "tag:mesh-llm";

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct TailscaleMeshPeer {
    pub hostname: String,
    pub address: String,
    pub os: Option<String>,
    pub models: Vec<String>,
    pub api_base_url: String,
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing)]
    pub invite_token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct TailscaleDoctorPeer {
    pub hostname: String,
    pub address: String,
    pub os: Option<String>,
    pub online: bool,
    pub reachable: bool,
    pub bootstrap_available: bool,
    pub latency_ms: Option<u64>,
    pub model_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct TailscaleDoctorReport {
    pub status_ok: bool,
    pub status_error: Option<String>,
    pub self_hostname: Option<String>,
    pub self_addresses: Vec<String>,
    pub total_peer_count: usize,
    pub online_peer_count: usize,
    pub tagged_peer_count: usize,
    pub online_tagged_peer_count: usize,
    pub reachable_tagged_peer_count: usize,
    pub bootstrap_peer_count: usize,
    pub peers: Vec<TailscaleDoctorPeer>,
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
    #[serde(rename = "Tags", default)]
    tags: Vec<String>,
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

struct PeerProbe {
    hostname: String,
    address: String,
    os: Option<String>,
    api_base_url: String,
    models: Vec<String>,
    invite_token: Option<String>,
    latency_ms: Option<u64>,
    online: bool,
    reachable: bool,
}

pub(crate) fn is_tailscale_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]),
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            segments[0] == 0xfd7a && segments[1] == 0x115c && segments[2] == 0xa1e0
        }
    }
}

pub(crate) fn is_known_tailscale_peer(ip: IpAddr) -> Result<bool> {
    let status = read_status()?;
    Ok(is_authorized_peer(&status.self_peer, ip)
        || status.peers.values().any(|peer| is_authorized_peer(peer, ip)))
}

fn is_authorized_peer(peer: &TailscalePeer, ip: IpAddr) -> bool {
    let address_matches = peer
        .addresses
        .iter()
        .filter_map(|address| address.parse::<IpAddr>().ok())
        .any(|address| address == ip);
    address_matches && peer.tags.iter().any(|tag| tag == REQUIRED_MESHLLM_TAILSCALE_TAG)
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
    let candidates = stream::iter(
        status
            .peers
            .into_values()
            .filter(|peer| {
                peer.online
                    && has_meshllm_tag(peer)
                    && !peer.addresses.iter().any(|addr| self_addresses.contains(addr))
                    && matches_target_name(peer, target_name)
            }),
    )
    .map(|peer| {
        let client = client.clone();
        async move { probe_peer(peer, client).await }
    })
    .buffer_unordered(TAILSCALE_PROBE_CONCURRENCY)
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .filter(|probe| probe.reachable)
    .map(|probe| TailscaleMeshPeer {
        hostname: probe.hostname,
        address: probe.address,
        os: probe.os,
        models: probe.models,
        api_base_url: probe.api_base_url,
        latency_ms: probe.latency_ms,
        invite_token: probe.invite_token,
    })
    .collect::<Vec<_>>();

    let mut candidates = candidates;
    candidates.sort_by(compare_peer_latency);
    Ok(candidates)
}

pub async fn doctor(timeout: Duration) -> TailscaleDoctorReport {
    let status = match read_status() {
        Ok(status) => status,
        Err(error) => {
            return TailscaleDoctorReport {
                status_ok: false,
                status_error: Some(error.to_string()),
                self_hostname: None,
                self_addresses: Vec::new(),
                total_peer_count: 0,
                online_peer_count: 0,
                tagged_peer_count: 0,
                online_tagged_peer_count: 0,
                reachable_tagged_peer_count: 0,
                bootstrap_peer_count: 0,
                peers: Vec::new(),
            };
        }
    };

    let total_peer_count = status.peers.len();
    let online_peer_count = status.peers.values().filter(|peer| peer.online).count();
    let tagged_peer_count = status.peers.values().filter(|peer| has_meshllm_tag(peer)).count();
    let online_tagged_peer_count = status
        .peers
        .values()
        .filter(|peer| peer.online && has_meshllm_tag(peer))
        .count();
    let self_hostname = peer_hostname(&status.self_peer);
    let self_addresses = status.self_peer.addresses.clone();

    let client = Client::builder()
        .timeout(timeout.max(DEFAULT_PROBE_TIMEOUT))
        .build();

    let mut status_error = None;
    let probes = match client {
        Ok(client) => {
            stream::iter(
                status
                    .peers
                    .into_values()
                    .filter(|peer| has_meshllm_tag(peer))
                    .filter(|peer| !peer.addresses.iter().any(|addr| self_addresses.contains(addr))),
            )
            .map(|peer| {
                let client = client.clone();
                async move { probe_peer(peer, client).await }
            })
            .buffer_unordered(TAILSCALE_PROBE_CONCURRENCY)
            .collect::<Vec<_>>()
            .await
        }
        Err(error) => {
            status_error = Some(format!(
                "Tailscale status is readable, but HTTP probing is unavailable: {error}"
            ));
            Vec::new()
        }
    };

    let reachable_tagged_peer_count = probes.iter().filter(|probe| probe.reachable).count();
    let bootstrap_peer_count = probes
        .iter()
        .filter(|probe| probe.reachable && probe.invite_token.is_some())
        .count();

    let mut peers = probes
        .into_iter()
        .map(|probe| TailscaleDoctorPeer {
            hostname: probe.hostname,
            address: probe.address,
            os: probe.os,
            online: probe.online,
            reachable: probe.reachable,
            bootstrap_available: probe.invite_token.is_some(),
            latency_ms: probe.latency_ms,
            model_count: probe.models.len(),
        })
        .collect::<Vec<_>>();
    peers.sort_by(compare_doctor_peer);

    TailscaleDoctorReport {
        status_ok: true,
        status_error,
        self_hostname,
        self_addresses,
        total_peer_count,
        online_peer_count,
        tagged_peer_count,
        online_tagged_peer_count,
        reachable_tagged_peer_count,
        bootstrap_peer_count,
        peers,
    }
}

async fn probe_peer(peer: TailscalePeer, client: Client) -> PeerProbe {
    let hostname = peer_hostname(&peer);
    let addresses = peer_ip_addresses(&peer);
    let fallback_address = addresses
        .first()
        .map(ToString::to_string)
        .unwrap_or_default();
    let fallback_api_base_url = addresses
        .first()
        .map(|ip| api_base_url(*ip))
        .unwrap_or_default();

    if !peer.online || addresses.is_empty() {
        return PeerProbe {
            hostname,
            address: fallback_address,
            os: peer.os,
            api_base_url: fallback_api_base_url,
            models: Vec::new(),
            invite_token: None,
            latency_ms: None,
            online: peer.online,
            reachable: false,
        };
    }

    for ip in addresses {
        let api_base_url = api_base_url(ip);
        let started = Instant::now();
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

        let latency_ms = elapsed_millis(started);
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

        return PeerProbe {
            hostname,
            address: ip.to_string(),
            os: peer.os,
            api_base_url,
            models,
            invite_token,
            latency_ms: Some(latency_ms),
            online: true,
            reachable: true,
        };
    }

    PeerProbe {
        hostname,
        address: fallback_address,
        os: peer.os,
        api_base_url: fallback_api_base_url,
        models: Vec::new(),
        invite_token: None,
        latency_ms: None,
        online: true,
        reachable: false,
    }
}

fn peer_ip_addresses(peer: &TailscalePeer) -> Vec<IpAddr> {
    let mut addresses = peer
        .addresses
        .iter()
        .filter_map(|address| address.parse::<IpAddr>().ok())
        .collect::<Vec<_>>();
    addresses.sort_by_key(|ip| (ip.is_ipv6(), ip.to_string()));
    addresses.dedup();
    addresses
}

fn api_base_url(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(ip) => format!("http://{ip}:{DEFAULT_MESH_API_PORT}"),
        IpAddr::V6(ip) => format!("http://[{ip}]:{DEFAULT_MESH_API_PORT}"),
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn compare_peer_latency(a: &TailscaleMeshPeer, b: &TailscaleMeshPeer) -> Ordering {
    match (a.latency_ms, b.latency_ms) {
        (Some(a), Some(b)) => a.cmp(&b).then_with(|| a.hostname.cmp(&b.hostname)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.hostname.cmp(&b.hostname),
    }
}

fn compare_doctor_peer(a: &TailscaleDoctorPeer, b: &TailscaleDoctorPeer) -> Ordering {
    match (a.latency_ms, b.latency_ms) {
        (Some(a), Some(b)) => a.cmp(&b).then_with(|| a.hostname.cmp(&b.hostname)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.hostname.cmp(&b.hostname),
    }
}

fn peer_hostname(peer: &TailscalePeer) -> String {
    if !peer.hostname.trim().is_empty() {
        peer.hostname.trim().to_string()
    } else {
        peer.dns_name.trim_end_matches('.').to_string()
    }
}

fn matches_target_name(peer: &TailscalePeer, target_name: Option<&str>) -> bool {
    let Some(target) = target_name else {
        return true;
    };

    let hostname = peer_hostname(peer);
    hostname.eq_ignore_ascii_case(target) || peer.dns_name.eq_ignore_ascii_case(target)
}

fn has_meshllm_tag(peer: &TailscalePeer) -> bool {
    peer.tags.iter().any(|tag| tag == REQUIRED_MESHLLM_TAILSCALE_TAG)
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
            latency_ms: Some(4),
            invite_token: Some("secret-invite-token".to_string()),
        };
        let json = serde_json::to_string(&peer).expect("peer should serialize");
        assert!(!json.contains("secret-invite-token"));
        assert!(!json.contains("invite_token"));
    }

    #[test]
    fn doctor_report_is_explicitly_non_secret() {
        let report = TailscaleDoctorReport {
            status_ok: true,
            status_error: None,
            self_hostname: Some("router".to_string()),
            self_addresses: vec!["100.64.0.1".to_string()],
            total_peer_count: 1,
            online_peer_count: 1,
            tagged_peer_count: 1,
            online_tagged_peer_count: 1,
            reachable_tagged_peer_count: 1,
            bootstrap_peer_count: 1,
            peers: vec![TailscaleDoctorPeer {
                hostname: "worker".to_string(),
                address: "100.64.0.2".to_string(),
                os: Some("linux".to_string()),
                online: true,
                reachable: true,
                bootstrap_available: true,
                latency_ms: Some(7),
                model_count: 2,
            }],
        };
        let json = serde_json::to_string(&report).expect("doctor report should serialize");
        assert!(!json.contains("invite_token"));
        assert!(!json.contains("secret"));
        assert!(json.contains("bootstrap_available"));
    }

    #[test]
    fn tailscale_status_decodes_peer_map() {
        let status: TailscaleStatus = serde_json::from_str(
            r#"{
                "Self":{"HostName":"router","TailscaleIPs":["100.64.0.1"],"Online":true,"Tags":["tag:mesh-llm"]},
                "Peer":{
                    "node-key":{"HostName":"worker","TailscaleIPs":["100.64.0.2"],"OS":"linux","Online":true,"Tags":["tag:mesh-llm","tag:other"]}
                }
            }"#,
        )
        .expect("status should decode");

        assert_eq!(status.self_peer.hostname, "router");
        assert_eq!(status.peers["node-key"].hostname, "worker");
        assert!(status.peers["node-key"].online);
        assert!(has_meshllm_tag(&status.peers["node-key"]));
    }

    #[test]
    fn peer_addresses_prefer_ipv4_but_keep_ipv6_fallback() {
        let peer: TailscalePeer = serde_json::from_str(
            r#"{"HostName":"worker","TailscaleIPs":["fd7a:115c:a1e0::2","100.64.0.2","100.64.0.2"]}"#,
        )
        .expect("peer should decode");

        let addresses = peer_ip_addresses(&peer);

        assert_eq!(
            addresses,
            vec![
                "100.64.0.2".parse::<IpAddr>().expect("valid IPv4"),
                "fd7a:115c:a1e0::2".parse::<IpAddr>().expect("valid IPv6"),
            ]
        );
        assert_eq!(
            api_base_url(addresses[1]),
            "http://[fd7a:115c:a1e0::2]:9337"
        );
    }

    #[test]
    fn target_name_matching_uses_hostname_or_dns_name() {
        let peer: TailscalePeer = serde_json::from_str(
            r#"{"HostName":"","DNSName":"Worker.example.ts.net.","TailscaleIPs":["100.64.0.2"]}"#,
        )
        .expect("peer should decode");

        assert!(matches_target_name(&peer, Some("worker")));
        assert!(matches_target_name(&peer, Some("worker.example.ts.net.")));
        assert!(!matches_target_name(&peer, Some("other")));
    }

    #[test]
    fn untagged_peers_are_not_meshllm_authorized() {
        let peer: TailscalePeer = serde_json::from_str(
            r#"{"HostName":"worker","TailscaleIPs":["100.64.0.2"],"Online":true,"Tags":["tag:other"]}"#,
        )
        .expect("peer should decode");

        assert!(!is_authorized_peer(
            &peer,
            "100.64.0.2".parse().expect("valid IP")
        ));
        assert!(!has_meshllm_tag(&peer));
    }

    #[test]
    fn meshllm_tag_authorizes_peer_by_exact_tag() {
        let peer: TailscalePeer = serde_json::from_str(
            r#"{"HostName":"worker","TailscaleIPs":["100.64.0.2"],"Online":true,"Tags":["tag:mesh-llm"]}"#,
        )
        .expect("peer should decode");

        assert!(is_authorized_peer(
            &peer,
            "100.64.0.2".parse().expect("valid IP")
        ));
        assert!(!is_authorized_peer(
            &peer,
            "100.64.0.3".parse().expect("valid IP")
        ));
    }
}
