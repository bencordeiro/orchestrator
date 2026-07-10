# Adding worker backends

Orchestrator treats every backend the same way: an OpenAI-compatible (or
Anthropic-native) endpoint saved as a **backend profile**, assignable to any
slot. There are three lanes, from most to least robust:

| Lane | Examples | Risk |
|------|----------|------|
| Local models | Ollama, vLLM, LM Studio, any OpenAI-compatible server | none |
| Official API keys (incl. subscription-included keys) | z.ai coding plan, DeepSeek, Mistral, OpenRouter, direct OpenAI/Anthropic/xAI keys | none — sanctioned |
| Subscription OAuth relay (CLIProxyAPI sidecar) | ChatGPT Plus, Antigravity, Grok, Kimi | ToS-gray; providers can break it anytime |

Prefer the first two lanes for daily workers. The relay lane is convenient but
can stop working without notice (Anthropic already blocks it for Claude).

## z.ai coding plan (recommended cheap worker)

z.ai's coding plan **officially includes API access**, which makes it one of the
best-value sanctioned workers available.

1. Log in at z.ai → Coding Plan dashboard → **API Keys** → create/copy a key.
2. In Orchestrator: **Backends → + Add manually**
   - **id**: `zai-glm`
   - **label**: `z.ai GLM (coding plan)`
   - **type**: OpenAI-compatible
   - **base URL**: `https://api.z.ai/api/coding/paas/v4`
   - **model**: `glm-4.7` (check your dashboard for current ids; an `-air`
     variant is available as a cheaper tier)
   - **auth ref**: `zai_api_key`
   - **API key**: paste it — stored in the OS keychain, never in a file
3. Save. The profile appears in every slot's backend dropdown.

Headless equivalent:

```
orchestrator secrets set zai_api_key <your-key>
```

then add the profile to `slots.json` with `"auth_ref": "zai_api_key"`.

## Ollama / local

Backends → Local models → **Scan** (probes `localhost:11434` plus any extra
hosts you list, e.g. another machine on your LAN) → **Add as profile** next to
any discovered model.

## Any other OpenAI-compatible provider

Same as the z.ai recipe with that provider's base URL, model id, and key. For
Anthropic direct keys choose type **Anthropic API** instead. The auth ref is
just a name you pick; the actual key always lives in the OS keychain.
