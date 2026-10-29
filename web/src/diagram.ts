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
function dark(): boolean {
  const asked = document.documentElement.dataset.theme;
  if (asked === 'light' || asked === 'dark') {
    return asked === 'dark';
  }
  return typeof matchMedia === 'function' && matchMedia('(prefers-color-scheme: dark)').matches;
}

/**
 * The colors of the page theme, as Mermaid names them. A token that the
 * page does not define is left out, and Mermaid keeps its own.
 */
export function themeVariables(): Record<string, string | boolean> {
  const style = getComputedStyle(document.documentElement);
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
    darkMode: dark(),
    fontFamily: 'system-ui, sans-serif',
    fontSize: '14px',
  };
  for (const [name, token] of Object.entries(tokens)) {
    const value = style.getPropertyValue(token).trim();
    if (value) {
      out[name] = value;
    }
  }
  return out;
}

/** Each drawing gets an id of its own: Mermaid scopes its styles by it. */
let drawn = 0;

/** Mermaid, set up for the page. */
async function ready(): Promise<Mermaid> {
  const mermaid = await load();
  const config: MermaidConfig = {
    startOnLoad: false,
    // No script, no click, and the labels escaped.
    securityLevel: 'strict',
    theme: 'base',
    themeVariables: themeVariables(),
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
 * The SVG of a source, safe to give to v-html. A source that Mermaid
 * refuses throws `Refused`. Any other error is one of the page.
 */
export async function draw(source: string): Promise<string> {
  const mermaid = await ready();

  let svg: string;
  try {
    // A parse fails with a short message, where a render may leave its
    // half-drawn work behind.
    await mermaid.parse(source);
    ({ svg } = await mermaid.render(`diagram-${(drawn += 1)}`, source));
  } catch (error) {
    throw new Refused(error instanceof Error ? error.message : String(error));
  }
  return DOMPurify.sanitize(svg, { USE_PROFILES: { svg: true, svgFilters: true } });
}

/**
 * Parse a source without drawing it, for a diagram that the page does not
 * show yet. A source that Mermaid refuses throws `Refused`.
 */
export async function check(source: string): Promise<void> {
  const mermaid = await ready();
  try {
    await mermaid.parse(source);
  } catch (error) {
    throw new Refused(error instanceof Error ? error.message : String(error));
  }
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
