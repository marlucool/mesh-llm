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
