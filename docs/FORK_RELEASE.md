# Marlucool MeshLLM fork release and installation

This document records the fork-specific release and installation behavior for
`marlucool/mesh-llm`.

## Release

The fork publishes its own GitHub releases. Installers therefore resolve
release assets from:

`marlucool/mesh-llm`

rather than from the upstream `Mesh-LLM/mesh-llm` repository.

The release pipeline builds the same composed host/runtime product format used by
the checked-in release workflow. A published release must contain the archive
and its `.sha256` sidecar so the installers can verify the download.

### Current fork release

> This release is produced from the fork's release branch; the final GitHub release is published by the release workflow after all required CI jobs pass.

- Version: `v0.76.1`
- Repository: `marlucool/mesh-llm`
- Release branch used to publish the release: `release/v0.76.1`
- Stable installer channel: `releases/latest`
- Prerelease installer channel: `releases?prerelease=true`

## Installation

### Linux/macOS

Use:

```bash
curl -fsSL https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.sh | bash
```

To select the latest published prerelease:

```bash
curl -fsSL https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.sh | bash -s -- --pre-release
```

The installer detects the operating system, architecture, and available backend,
selects the corresponding release asset, downloads its SHA-256 sidecar, verifies
the archive, and installs the `mesh-llm` bundle.

### Windows

Use PowerShell:

```powershell
irm https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.ps1 | iex
```

For the latest published prerelease:

```powershell
irm https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.ps1 -OutFile install.ps1
.\install.ps1 -PreRelease
```

The Windows installer targets the x86_64 product bundle and verifies the
download when a checksum sidecar is published.

## Why installation previously failed

The fork had installer logic that was already configured to resolve
`marlucool/mesh-llm`, but the fork did not yet have a published release.
Consequently the installer requested:

`https://github.com/marlucool/mesh-llm/releases/latest/download/<asset>`

and GitHub had no release asset at that location.

This is a distribution/release-state problem, not a missing command-line
argument. Publishing the fork release is required for the normal installer
path to work.

## Release workflow change

The release workflow now supports both:

1. the existing manual `workflow_dispatch` path; and
2. a `release/**` branch push path used by the fork release process.

The workflow still performs its normal release validation and publishing stages.
The branch-triggered path exists so the fork can publish a release using the
repository's existing CI release graph without requiring a separate local
release implementation.

The release workflow:

- validates the semantic version;
- verifies that the source commit is reachable from `main`;
- builds the host and native-runtime inputs;
- composes the product bundles;
- runs release smoke tests;
- generates runtime manifests and checksums;
- publishes the GitHub release and release assets;
- generates release notes;
- invokes downstream packaging only when the required stable-release conditions
  are met.

## Installer repository selection

Both installers default to:

```text
marlucool/mesh-llm
```

They can also be pointed at another repository for testing with:

### Bash

```bash
MESH_LLM_INSTALL_REPO=owner/repository curl -fsSL \
  https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.sh | bash
```

### PowerShell

```powershell
$env:MESH_LLM_INSTALL_REPO = "owner/repository"
irm https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.ps1 | iex
```

For a controlled test of a fixed release asset base, the installers also expose
`MESH_LLM_INSTALL_URL_BASE`.

## Checksums

Release archives are accompanied by SHA-256 sidecars:

```text
<asset>.tar.gz.sha256
<asset>.zip.sha256
```

The installer compares the downloaded archive against the digest in the
sidecar. Set `MESH_LLM_REQUIRE_CHECKSUM=1` to make a missing sidecar a hard
failure instead of allowing the installer to continue with a warning.

## Post-install setup

After the executable is installed, the installer normally runs:

```bash
mesh-llm setup
```

On Windows this is:

```powershell
mesh-llm.exe setup
```

Use `--no-setup` when you need to separate executable installation from
runtime setup.

For a persistent worker that should return after a machine restart, the
background service path is:

```bash
mesh-llm setup --service
```

The setup/service behavior is documented separately in the normal MeshLLM
operator documentation.

## Fork-specific changes already present

The fork's recent maintenance includes:

- installer metadata pointing at `marlucool/mesh-llm`;
- fork README installation commands pointing at the fork;
- Tailscale/private-network integration and its associated discovery/setup
  documentation;
- service/reconnect behavior intended for nodes that should remain available
  after restarts;
- fork/upstream synchronization documentation;
- the release workflow change documented above.

This document deliberately does not claim that upstream features are fork-only.
For upstream functionality, use the corresponding MeshLLM documentation and
the fork's commit history to identify whether a change originated in the fork
or was synchronized from upstream.

## Verifying a published release

After the release workflow finishes, verify that the fork has:

1. a `v0.76.1` tag;
2. a non-draft GitHub release for that tag;
3. platform archives matching the release matrix;
4. a `.sha256` file for each archive;
5. the `latest` installer URL resolving to an actual asset.

A successful Linux smoke test is:

```bash
curl -fsSL https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.sh | bash
mesh-llm --version
```

A successful Windows smoke test is:

```powershell
irm https://raw.githubusercontent.com/marlucool/mesh-llm/main/install.ps1 | iex
mesh-llm.exe --version
```

If an installer reports a 404 for a release asset, check the GitHub Releases
page first. The installer intentionally depends on the release artifact contract;
it should not silently fall back to an arbitrary source build.
