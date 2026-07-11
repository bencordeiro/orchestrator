# Agent setup — connect any LLM/CLI to Orchestrator

Two steps: (1) register the MCP server once per machine, (2) give your agent
the usage prompt below. Get your real endpoint + bearer token from the app:
**Settings → Connect your agents** (one-click copy).

## 1. Register the MCP server

**Claude Code**

```
claude mcp add --transport http orchestrator http://localhost:7420/mcp --header "Authorization: Bearer <TOKEN>"
```

**Codex CLI**

```
codex mcp add orchestrator -- npx -y mcp-remote http://localhost:7420/mcp --header "Authorization: Bearer <TOKEN>"
```

**Any other MCP client** (project `.mcp.json` or equivalent config):

```json
{
  "mcpServers": {
    "orchestrator": {
      "type": "http",
      "url": "http://localhost:7420/mcp",
      "headers": { "Authorization": "Bearer <TOKEN>" }
    }
  }
}
```

> Keep files containing the token out of version control (this repo's
> `.gitignore` already excludes `.mcp.json`).

## 2. The usage prompt

Paste this into your agent's standing instructions — `CLAUDE.md`, `AGENTS.md`,
a system prompt, or just the chat. It works with any model:

---

You have access to an MCP server called **orchestrator**. It gives you a team
of worker models you can delegate tasks to. Use it like this:

- Call `list_slots` once to see the available workers and what each is for.
  Slot names are stable; the model behind a slot may change at any time —
  never assume which vendor or model is answering.
- Delegate real work with `delegate(task, slot?, conversation_id?, context?)`.
  The default slot is `worker`.
- **Workers are context-blind.** They cannot see this conversation, your
  files, or your tools. Every delegation must be a complete, self-contained
  brief: state the goal, include all necessary code/data inline (use the
  `context` field for bulk material), specify the exact output format you
  want, and give concrete acceptance criteria. Be concise, but leave nothing
  to be inferred.
- For anything that may generate for more than ~1 minute, pass
  **`background: true`** — you get a `job_id` back instantly and keep working;
  fetch the result later with `job_result(job_id)`. Poll occasionally (not in
  a tight loop). This is immune to tool-call timeouts. Never cancel-and-retry
  a foreground call that seems slow; long waits are normal generation time.
- Prefer **fresh stateless jobs** (omit `conversation_id`) for independent
  tasks. Pass a previous `conversation_id` only when the worker genuinely
  needs to remember its own earlier output (e.g. draft → critique → revise);
  the full thread history is re-sent on every continued call, so threads cost
  more.
- Delegate the heavy lifting: implementation, boilerplate, long transformations,
  first drafts, summarization. Keep judgment, planning, and final review for
  yourself. Verify worker output before relying on it.
- If a delegation returns "worker unavailable", report it to the user and
  wait — the user swaps the backend in a GUI; do not retry in a loop and do
  not try to route around it yourself.

---

## Troubleshooting

- `401` on connect → stale token; re-copy the setup command from Settings.
- "worker unavailable" → the slot's backend is down/out of quota; swap the
  backend in the app (Slots tab) and delegate again — same session keeps working.
- Prompt the current worker directly (bypassing MCP) to isolate backend issues:
  `powershell -File scripts\test-worker.ps1 "say hi"`.
