// Per-turn agent runner. stdin: one JSON request. stdout: JSON-lines events
// (delta | tool | done | error). Spawned by Xuewen's AgentService; exits
// when the turn ends. See docs/superpowers/specs/2026-07-16-*-design.md.
import { readFileSync } from 'node:fs';
import { runClaude } from './claude.mjs';
import { runCodex } from './codex.mjs';
import { emit } from './protocol.mjs';

const req = JSON.parse(readFileSync(0, 'utf8'));
try {
  if (req.backend === 'claude_code') await runClaude(req);
  else if (req.backend === 'codex') await runCodex(req);
  else throw new Error(`unknown backend: ${req.backend}`);
  emit({ type: 'done' });
} catch (e) {
  emit({ type: 'error', message: String(e?.message ?? e) });
  process.exitCode = 1;
}
