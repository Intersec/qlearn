// The diagrams of a page, drawn by Mermaid.
//
// Mermaid is the largest dependency of the interface, so it loads the first
// time that a page holds a diagram, in a chunk of its own (section 13 of
// the design). The agent writes the source: Mermaid runs in its strict
// mode, and the SVG goes through DOMPurify before it enters the page.

import DOMPurify from 'dompurify';
import type { Mermaid, MermaidConfig } from 'mermaid';

/** A source that Mermaid refuses. The agent wrote it, so the agent fixes it. */
export class Refused extends Error {}

let loading: Promise<Mermaid> | null = null;

function load(): Promise<Mermaid> {
  loading ??= import('mermaid').then(
    (module) => module.default,
    (error: unknown) => {
      // A chunk that failed to load is tried again by the next diagram.
      loading = null;
      throw error;
    },
  );
  return loading;
}

/** Whether the page shows its dark theme, by the rules of main.css. */
export function dark(): boolean {
  const asked = document.documentElement.dataset.theme;
  if (asked === 'light' || asked === 'dark') {
    return asked === 'dark';
  }
  return typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches;
}

/**
 * Where the colors of a drawing come from: the tokens of the page theme,
 * by name, and whether that theme is dark.
 */
export interface Palette {
  dark: boolean;
  /** The value of a token such as `--ink`, or "" when it is not defined. */
  token(name: string): string;
}

/** The theme that the page shows now. */
export function shown(): Palette {
  const style = getComputedStyle(document.documentElement);
  return { dark: dark(), token: (name) => style.getPropertyValue(name).trim() };
}

/**
 * The colors of a palette, as Mermaid names them. A token that the palette
 * does not define is left out, and Mermaid keeps its own.
 */
export function themeVariables(palette: Palette = shown()): Record<string, string | boolean> {
  const tokens: Record<string, string> = {
    background: '--page',
    primaryColor: '--agent',
    primaryBorderColor: '--action',
    primaryTextColor: '--ink',
    secondaryColor: '--pane',
    secondaryBorderColor: '--edge',
    secondaryTextColor: '--ink',
    tertiaryColor: '--pane',
    tertiaryBorderColor: '--edge',
    tertiaryTextColor: '--ink',
    mainBkg: '--agent',
    nodeBorder: '--action',
    nodeTextColor: '--ink',
    textColor: '--ink',
    titleColor: '--ink',
    lineColor: '--ink-quiet',
    clusterBkg: '--pane',
    clusterBorder: '--edge',
    edgeLabelBackground: '--page',
    noteBkgColor: '--marked',
    noteBorderColor: '--marked-edge',
    noteTextColor: '--ink',
    actorBkg: '--agent',
    actorBorder: '--action',
    actorTextColor: '--ink',
    actorLineColor: '--ink-quiet',
    signalColor: '--ink-quiet',
    signalTextColor: '--ink',
    labelBoxBkgColor: '--pane',
    labelBoxBorderColor: '--edge',
    labelTextColor: '--ink',
    loopTextColor: '--ink',
    activationBkgColor: '--pane',
    activationBorderColor: '--edge',
  };

  const out: Record<string, string | boolean> = {
    darkMode: palette.dark,
    fontFamily: 'system-ui, sans-serif',
    fontSize: '14px',
  };
  for (const [name, token] of Object.entries(tokens)) {
    const value = palette.token(token);
    if (value) {
      out[name] = value;
    }
  }
  return out;
}

/** Each drawing gets an id of its own: Mermaid scopes its styles by it. */
let drawn = 0;

/** The end of the last job of Mermaid. */
let queue: Promise<unknown> = Promise.resolve();

/**
 * Run a job of Mermaid once the last one ends. Mermaid queues a parse and
 * a render, but `initialize` sets its global config at once, and a render
 * reads it when it runs: a drawing in another palette, such as the light
 * one of a saved file, must not start between them.
 */
function serial<T>(job: () => Promise<T>): Promise<T> {
  const run = queue.then(job);
  queue = run.catch(() => {});
  return run;
}

/** Mermaid, set up with the colors of a palette. */
async function ready(palette: Palette = shown()): Promise<Mermaid> {
  const mermaid = await load();
  const config: MermaidConfig = {
    startOnLoad: false,
    // No script, no click, and the labels escaped.
    securityLevel: 'strict',
    theme: 'base',
    themeVariables: themeVariables(palette),
    // Labels as SVG text: the SVG profile of DOMPurify drops the HTML of a
    // foreignObject.
    htmlLabels: false,
    // An error goes back to the agent: the console of the learner stays
    // quiet, and the body gets no error drawing.
    logLevel: 'fatal',
    suppressErrorRendering: true,
  };
  mermaid.initialize(config);
  return mermaid;
}

/**
 * The SVG of a source, safe to give to v-html, in the colors of the page
 * or of another palette. A source that Mermaid refuses throws `Refused`.
 * Any other error is one of the page.
 */
export async function draw(source: string, palette?: Palette): Promise<string> {
  const svg = await serial(async () => {
    const mermaid = await ready(palette);
    try {
      // A parse fails with a short message, where a render may leave its
      // half-drawn work behind.
      await mermaid.parse(source);
      return (await mermaid.render(`diagram-${(drawn += 1)}`, source)).svg;
    } catch (error) {
      throw new Refused(error instanceof Error ? error.message : String(error));
    }
  });
  return DOMPurify.sanitize(svg, { USE_PROFILES: { svg: true, svgFilters: true } });
}

/**
 * Above this width in pixels, a diagram scaled into one column of the
 * cheatsheet would draw its 14px text below 11px.
 */
export const WIDE = 400;

/** The width that Mermaid drew, from the viewBox of its SVG, or null. */
export function naturalWidth(svg: string): number | null {
  const box = /<svg\b[^>]*\sviewBox="[-\d.e]+[\s,]+[-\d.e]+[\s,]+([\d.e]+)[\s,]+[\d.e]+"/i.exec(
    svg,
  );
  const width = box ? Number(box[1]) : NaN;
  return Number.isFinite(width) ? width : null;
}

/**
 * Parse a source without drawing it, for a diagram that the page does not
 * show yet. A source that Mermaid refuses throws `Refused`.
 */
export function check(source: string): Promise<void> {
  return serial(async () => {
    const mermaid = await ready();
    try {
      await mermaid.parse(source);
    } catch (error) {
      throw new Refused(error instanceof Error ? error.message : String(error));
    }
  });
}

/**
 * Call `changed` when the page changes its theme: the system turns dark, or
 * the root asks for a theme. Answers what stops it.
 */
export function watchTheme(changed: () => void): () => void {
  const media =
    typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null;
  media?.addEventListener('change', changed);
  const observer = new MutationObserver(changed);
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme'],
  });

  return () => {
    media?.removeEventListener('change', changed);
    observer.disconnect();
  };
}
