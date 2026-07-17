// Test stand-in for agent-runner/src/runner.mjs: same protocol, canned
// output keyed off the question, no SDKs, no network.
import { readFileSync } from 'node:fs';
const req = JSON.parse(readFileSync(0, 'utf8'));
const say = (o) => process.stdout.write(JSON.stringify(o) + '\n');
if (req.question.includes('fail')) {
  say({ type: 'error', message: 'boom' });
  process.exit(1);
} else if (req.question.includes('hang')) {
  setTimeout(() => {}, 60_000); // never answers; exercises the turn timeout
} else if (req.question.includes('die')) {
  process.stderr.write('stub exploded\n');
  process.exit(3); // exits without done/error
} else {
  say({ type: 'tool', name: 'Read', detail: 'paper.txt' });
  say({ type: 'delta', text: 'Hel' });
  say({ type: 'delta', text: `lo from ${req.backend}${req.hasRepo ? ' with repo' : ''}` });
  say({ type: 'done' });
}
