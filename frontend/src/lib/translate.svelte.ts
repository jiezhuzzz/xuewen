import { translateText } from './api';
import { appSettings } from './state.svelte';

type Provider = 'llm' | 'deepl';

/// Shared state for the single app-level translate popover. Only one
/// translation is ever in flight/shown, so a lone `$state` record drives the
/// root-mounted `TranslatePopover.svelte` (Task 6) the same way `contextMenu`
/// drives `PaperContextMenu.svelte`.
export const translateBox = $state<{
  open: boolean;
  x: number;
  y: number;
  source: string;
  translation: string;
  provider: Provider | null;
  loading: boolean;
  error: string | null;
}>({ open: false, x: 0, y: 0, source: '', translation: '', provider: null, loading: false, error: null });

const STORAGE_KEY = 'xuewen.translateMode';

function initialMode(): 'auto' | 'manual' {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === 'auto' || v === 'manual') return v;
  } catch {
    /* no localStorage (SSR/test) */
  }
  return appSettings.translate.trigger ?? 'auto';
}

/// The user's selection-translate trigger preference, persisted in
/// localStorage and seeded from the server's `[translate].trigger` default.
export const translateMode = $state<{ value: 'auto' | 'manual' }>({ value: initialMode() });

export function setTranslateMode(m: 'auto' | 'manual'): void {
  translateMode.value = m;
  try {
    localStorage.setItem(STORAGE_KEY, m);
  } catch {
    /* ignore */
  }
}

/// Reconciles `translateMode` against the server's `[translate].trigger`
/// default once settings have loaded. `initialMode()` runs at module
/// evaluation time — before the async `loadSettings()` populates
/// `appSettings.translate.trigger` — so a first-time user (no stored
/// localStorage pref) always got the hardcoded 'auto' fallback. Call this
/// after `loadSettings()` resolves to apply the server default; a user who
/// has ever explicitly chosen a mode (stored in localStorage) keeps it.
export function syncTranslateModeFromSettings(): void {
  try {
    if (localStorage.getItem(STORAGE_KEY) !== null) return; // explicit stored pref — keep it
  } catch {
    /* no localStorage */
  }
  const t = appSettings.translate.trigger;
  if (t === 'auto' || t === 'manual') translateMode.value = t;
}

// Guards against a slower, superseded request clobbering a newer one's
// result once both resolve out of order.
let seq = 0;

export async function requestTranslate(text: string, at: { x: number; y: number }, provider?: Provider): Promise<void> {
  const my = ++seq;
  translateBox.open = true;
  translateBox.x = at.x;
  translateBox.y = at.y;
  translateBox.source = text;
  translateBox.translation = '';
  translateBox.provider = provider ?? appSettings.translate.default_provider ?? null;
  translateBox.loading = true;
  translateBox.error = null;
  try {
    const r = await translateText(text, {
      provider,
      targetLang: appSettings.translate.target_lang,
    });
    if (my !== seq) return; // superseded by a newer request
    translateBox.translation = r.translation;
    translateBox.provider = (r.provider as Provider) ?? translateBox.provider;
  } catch (e) {
    if (my === seq) translateBox.error = (e as Error).message;
  } finally {
    if (my === seq) translateBox.loading = false;
  }
}

export function closeTranslate(): void {
  seq++; // invalidate any in-flight request
  translateBox.open = false;
  translateBox.translation = '';
  translateBox.source = '';
  translateBox.error = null;
  translateBox.loading = false;
  translateBox.x = 0;
  translateBox.y = 0;
  translateBox.provider = null;
}
