<!--
SPDX-FileCopyrightText: Copyright (c) 2026 Mesh-LLM contributors
SPDX-License-Identifier: Apache-2.0
-->

# Tailscale networking

MeshLLM can use Tailscale as a private network underneath its existing mesh
transport. No Tailscale-specific transport is required in the protocol: the
node's normal networking stack can reach peers over their Tailscale addresses.

This keeps Tailscale optional while making it a supported deployment path for
PAIR + MeshLLM installations.

## Recommended topology

For a normal multi-PC deployment, install Tailscale directly on every MeshLLM
host:

```
PC / GPU node A  -- Tailscale --+
PC / GPU node B  -- Tailscale --+-- private tailnet
PC / GPU node C  -- Tailscale --+
```

MeshLLM remains responsible for mesh identity, discovery, model routing, and
inference. Tailscale provides private IP connectivity between the hosts.

This is preferable to making MeshLLM depend on Tailscale because the same
MeshLLM binary continues to work on ordinary networks.

## MeshLLM authorization tag


When Tailscale discovery mode is enabled, MeshLLM only considers peers that
carry the dedicated non-human-device tag `tag:mesh-llm`. This is an application
authorization boundary in addition to ordinary Tailscale network connectivity.
A device being a member of the tailnet, or merely having a 100.x Tailscale IP,
does not make it an automatic MeshLLM worker.

Assign `tag:mesh-llm` to each machine that is intended to run MeshLLM. Keep the
tag in the tailnet policy under `tagOwners`, and grant the tag only the network
access required by the MeshLLM deployment. For discovery and automatic join,
the MeshLLM API normally needs TCP port `9337`:

```jsonc
{
  "tagOwners": {
    "tag:mesh-llm": []
  },
  "grants": [
    {
      "src": ["tag:mesh-llm"],
      "dst": ["tag:mesh-llm"],
      "ip": ["tcp:9337"]
    }
  ]
}
```

The exact `tagOwners` entry should follow your existing tailnet ownership
policy; the example above leaves tag management to tailnet administrators.
Tailscale tags are intended for non-human devices and also identify the device
for access-control purposes. citeturn758209search6turn758209search5

MeshLLM checks the local Tailscale control-plane peer state before returning its
automatic join bootstrap token. The caller must both match a currently known
Tailscale peer address and carry `tag:mesh-llm`. The normal MeshLLM invite-token
and membership system remains the second authentication layer.

This means the intended trust chain is:

```text
Tailscale connectivity
        ↓
`tag:mesh-llm` authorization
        ↓
MeshLLM automatic bootstrap
        ↓
existing MeshLLM invite-token authentication
        ↓
normal mesh membership
```

Tailscale's GitHub Action follows the same principle: CI runners receive an
explicit tag and are governed by the grants attached to that tag. citeturn758209search0turn758209search3
## Tailscale addresses

When diagnosing a node, check its Tailscale IPv4 address with:

```bash
tailscale ip -4
```

You can also verify the peer is reachable:

```bash
tailscale ping <peer-name>
```

Use MagicDNS names when available instead of hard-coding 100.x addresses in
configuration.

## Proxmox

There are two different deployment cases:

1. **Physical GPU host:** install Tailscale directly on the host running
   MeshLLM.
2. **Proxmox server hosting a guest:** install Tailscale inside the Ubuntu
   guest that runs MeshLLM. This avoids making MeshLLM depend on the Proxmox
   host OS.

A subnet router is useful when a device cannot run Tailscale itself. It is not
normally necessary when every MeshLLM node can run the Tailscale client.

Userspace networking is another option for restricted environments where a
normal TUN interface cannot be used.

## Security

Do not expose MeshLLM's service ports to the public internet merely because
Tailscale is enabled. Keep the service bound according to the normal MeshLLM
deployment and use Tailscale ACLs/tags to control which machines can reach
which nodes.

Tailscale is a network path, not an application authorization layer. MeshLLM's
own identity and invite/authentication mechanisms remain authoritative.

## GitHub Actions

For integration tests that need to reach a private MeshLLM node, use
Tailscale's GitHub Action with an ephemeral tagged identity. Do not put a
long-lived personal Tailscale credential directly into a workflow file.

A repository can configure:

- `TS_OAUTH_CLIENT_ID`
- `TS_AUDIENCE`

and connect a hosted runner to the tailnet for the duration of a test.

The runner should receive a dedicated tag with the minimum ACL permissions
needed for the test.

## Environment-file example

The generated MeshLLM service environment file can contain deployment-specific
values such as:

```text
# Optional: document the private network used by this node.
# MESH_LLM_NETWORK=tailscale
# MESH_LLM_TAILSCALE_HOST=node-a.example.ts.net
```

These values are intentionally informational today. MeshLLM does not require
them to operate; the OS networking layer and existing mesh transport continue
to determine connectivity.

## Why this is an integration rather than a new transport

MeshLLM already has its own authenticated mesh protocol and transport. Adding a
second application-level Tailscale protocol would duplicate networking logic
and make deployments harder.

The integration point is therefore deployment and connectivity:

MeshLLM protocol -> existing transport -> Tailscale interface -> private tailnet

That also means a node can move between direct networking, a Tailscale network,
or another private network without changing its MeshLLM identity or model
configuration.
