# MeshLLM fork upstream sync

This fork keeps its custom MeshLLM changes on top of the upstream Mesh-LLM/mesh-llm project.

## Automatic sync

The repository runs the sync job every six hours and also exposes it under
**Actions → Sync with Mesh-LLM upstream → Run workflow**.

The job:

1. fetches the current `Mesh-LLM/mesh-llm` `main`;
2. fast-forwards when the fork is simply behind;
3. creates a normal merge commit when the fork contains fork-only commits;
4. pushes the result to the fork's `main`.

The workflow never force-pushes `main`. When upstream and fork changes conflict,
the job aborts before pushing anything and reports that a dedicated conflict
resolution is required.

## Manual sync

```bash
git remote add upstream https://github.com/Mesh-LLM/mesh-llm.git
git fetch upstream main
git checkout main
git merge --no-edit --no-ff upstream/main
git push origin main
```

After a conflict, resolve and test the merge before pushing.

## Why this is separate from feature branches

Fork-specific work should land in `main` first. Feature branches such as
`pair-mixed-gpu-integration` are not automatically rebased by the sync job,
which avoids rewriting active development branches during routine maintenance.
