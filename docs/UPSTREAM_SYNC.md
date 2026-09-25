# Upstream synchronization

This fork tracks Mesh-LLM/mesh-llm on the main branch.

## Automatic sync

The .github/workflows/sync-upstream.yml workflow runs every six hours and can also be started manually from GitHub Actions.

When upstream has new commits, the workflow attempts a normal merge into this fork's main branch. Fork-only changes are preserved.

A clean merge is pushed automatically. A conflicting merge is aborted without changing main; the workflow summary reports that manual conflict resolution is required. This prevents an upstream update from silently overwriting fork-specific work.

## Manual sync

From a local clone:

    git fetch upstream main
    git checkout main
    git merge --no-edit --no-ff upstream/main
    git push origin main

Add the upstream remote once if it is missing:

    git remote add upstream https://github.com/Mesh-LLM/mesh-llm.git

## Fork-specific code

The fork's MeshLLM/Tailscale integration, terminal dashboard entrypoint, persistent explicit join token, and Windows logon startup are fork-local changes. A normal merge keeps them in place unless upstream changes the same lines and a conflict must be resolved.

When resolving a conflict, keep the upstream behavior where it is unrelated to the fork feature and reapply the fork-specific behavior only where the two changes intentionally overlap.
