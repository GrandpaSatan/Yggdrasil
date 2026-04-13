# Explore the Flows

Flows are multi-step pipelines Odin runs across your model fleet. The headline flow is `coding_swarm`:

1. **generate** — code from `nemotron-3-nano:4b` on Munin
2. **review** — bug review from `gemma4:e4b` on Hugin (different architecture catches different bugs)
3. **refine** — corrections back through Nemotron, looping until the reviewer says LGTM

**What to do:**
1. Click the **Yggdrasil tree icon** in the activity bar (left side)
2. Expand "Coding Flows" → click `coding_swarm`
3. The Flows Explorer opens — read the Topology tab, then click the `coding_swarm` tab
4. Click "view prompt" on any step to see the exact `system_prompt` and `input_template` it uses

You can also press `Ctrl+Shift+Y` any time to open the Flows Explorer.
