import type { Preferences } from "../ipc";

/**
 * Push preferences into CSS custom properties on `:root`.
 *
 * Themeing happens here rather than through a provider and a thousand styled
 * components: one accent value lands on `--accent`, and every tint in the app
 * is `color-mix`ed from it in CSS. Changing colour is one assignment, not a
 * re-render.
 */
export function applyTheme(prefs: Preferences, accentForeground: string) {
  const root = document.documentElement;
  root.style.setProperty("--accent", prefs.appearance.accent);
  root.style.setProperty("--accent-ink", accentForeground);
  root.dataset.glass = prefs.appearance.glass;
  root.dataset.reduceMotion = String(prefs.appearance.reduceMotion);
}
