import { beforeEach, describe, expect, it, vi } from 'vitest';
import { requestTranslate, translateBox, closeTranslate } from './translate.svelte';

beforeEach(() => {
  closeTranslate();
  vi.unstubAllGlobals();
});

describe('requestTranslate', () => {
  it('opens the box, shows loading, then fills the translation', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({ translation: '你好', provider: 'llm', source_lang: 'EN', target_lang: 'zh' }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    );
    await requestTranslate('hello', { x: 100, y: 200 });
    expect(translateBox.open).toBe(true);
    expect(translateBox.translation).toBe('你好');
    expect(translateBox.provider).toBe('llm');
    expect(translateBox.loading).toBe(false);
    expect(translateBox.error).toBeNull();
  });

  it('records an error when the request fails', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('nope', { status: 502 })));
    await requestTranslate('hello', { x: 0, y: 0 });
    expect(translateBox.open).toBe(true);
    expect(translateBox.error).not.toBeNull();
    expect(translateBox.loading).toBe(false);
  });
});
