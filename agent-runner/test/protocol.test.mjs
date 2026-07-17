import test from 'node:test';
import assert from 'node:assert/strict';
import { composePrompt, toolDetail } from '../src/protocol.mjs';

test('composePrompt lists workspace contents, paper, transcript, question', () => {
  const p = composePrompt({
    hasRepo: true,
    paper: { title: 'Attention Is All You Need', venue: 'NeurIPS', year: 2017, authors: ['Vaswani'] },
    transcript: [
      { role: 'user', content: 'hi' },
      { role: 'assistant', content: 'hello' },
    ],
    question: 'where is the mask applied?',
  });
  assert.match(p, /paper\.txt/);
  assert.match(p, /repo\//);
  assert.match(p, /Attention Is All You Need/);
  assert.match(p, /Authors: Vaswani/);
  assert.match(p, /Researcher: hi/);
  assert.match(p, /You \(earlier\): hello/);
  assert.match(p, /Researcher: where is the mask applied\?$/);
});

test('composePrompt omits the repo line when none is attached', () => {
  const p = composePrompt({ hasRepo: false, paper: { title: 'T' }, transcript: [], question: 'q' });
  assert.doesNotMatch(p, /repo\//);
});

test('toolDetail picks a representative input field and truncates', () => {
  assert.equal(toolDetail({ file_path: 'paper.txt' }), 'paper.txt');
  assert.equal(toolDetail({ pattern: 'mask' }), 'mask');
  assert.equal(toolDetail(null), '');
  assert.equal(toolDetail({ file_path: 'x'.repeat(200) }).length, 120);
});
