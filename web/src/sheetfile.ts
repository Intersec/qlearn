// The cheatsheet as one HTML file that stands alone (section 9 of the
// design). The page builds it, because only the page has the drawn
// diagrams and charts: the file is a copy of the sheet on the screen.
//
// The file runs no script and loads nothing. It keeps the light theme,
// since it is read outside the app and printed. The decisions are in the
// entry "Holes found by the HTML export" of roadmap/stack.md.

import type { Block, Cheatsheet, ImageBlock } from '@/api/types';
import { Refused, WIDE, draw, naturalWidth, type Palette } from '@/diagram';

/** The bytes of images that one file holds at most, before base64. */
export const MEDIA_BUDGET = 8 * 1024 * 1024;

/** What the file needs from the server and from Mermaid. */
export interface Sources {
  /** The SVG of a Mermaid source, in the colors of a palette. */
  draw(source: string, palette: Palette): Promise<string>;
  /** The bytes of a file of `media/`, or null when the server sends none. */
  media(name: string): Promise<Blob | null>;
}

const SERVER: Sources = {
  draw,
  async media(name) {
    // A request of the same origin carries the cookie of the token.
    const answer = await fetch(`/api/media/${encodeURIComponent(name)}`);
    return answer.ok ? answer.blob() : null;
  },
};

/** The name of the file, as the Markdown export names its own. */
export function fileName(slug: string): string {
  return `${slug}-cheatsheet.html`;
}

/**
 * The file of the sheet that the page shows in `sheet`, the element of
 * class `cheatsheet-sheet`. `sheets` are the stylesheets of the page.
 */
export async function sheetFile(
  sheet: HTMLElement,
  cheatsheet: Cheatsheet,
  title: string,
  sources: Sources = SERVER,
  sheets: Iterable<CSSStyleSheet> = document.styleSheets,
): Promise<string> {
  const doc = document.implementation.createHTMLDocument(title);
  doc.documentElement.lang = 'en';
  doc.head.prepend(
    meta(doc, { charset: 'utf-8' }),
    meta(doc, { name: 'viewport', content: 'width=device-width, initial-scale=1' }),
    // The colors of the file are the light ones, whatever the system says.
    meta(doc, { name: 'color-scheme', content: 'light' }),
  );
  const article = doc.createElement('article');
  article.className = 'cheatsheet';
  article.setAttribute('aria-label', 'Cheatsheet');
  const copy = doc.importNode(sheet, true);
  article.append(copy);
  doc.body.append(article);

  const tokens = lightTokens(sheets);
  const blocks = blockElements(copy, cheatsheet.blocks);
  await redraw(blocks, { dark: false, token: (name) => tokens.get(name) ?? '' }, sources);
  await embed(blocks, sources);
  resolve(copy, tokens);
  sweep(copy);

  const style = doc.createElement('style');
  style.textContent = css(sheets, doc);
  doc.head.append(style);
  return `<!doctype html>\n${doc.documentElement.outerHTML}\n`;
}

function meta(doc: Document, attributes: Record<string, string>): HTMLMetaElement {
  const element = doc.createElement('meta');
  for (const [name, value] of Object.entries(attributes)) {
    element.setAttribute(name, value);
  }
  return element;
}

/** Each block of the cheatsheet with its element in the copy. */
function blockElements(copy: Element, blocks: Block[]): [Block, Element][] {
  const elements = new Map<string, Element>();
  for (const element of copy.querySelectorAll('[data-block]')) {
    elements.set(element.getAttribute('data-block') ?? '', element);
  }
  return blocks.flatMap((block): [Block, Element][] => {
    const element = elements.get(block.id);
    return element ? [[block, element]] : [];
  });
}

/**
 * Draw every diagram again in the light theme: Mermaid writes its colors
 * into the SVG, and the page may show the dark one. A new drawing gets a
 * new id, so the ids of the file stay unique. A diagram that Mermaid
 * refuses shows its error, as on the screen.
 */
async function redraw(blocks: [Block, Element][], palette: Palette, sources: Sources) {
  const figures: [Element, string][] = [];
  for (const [block, element] of blocks) {
    const figure = element.querySelector('.diagram');
    if (block.kind === 'diagram' && figure) {
      figures.push([figure, block.source]);
    }
    if (block.kind === 'steps') {
      for (const [n, step] of block.steps.steps.entries()) {
        const inStep = element.querySelector(`[data-step="${n}"] .diagram`);
        if (step.diagram !== null && inStep) {
          figures.push([inStep, step.diagram]);
        }
      }
    }
  }

  for (const [figure, source] of figures) {
    let svg: string;
    try {
      svg = await sources.draw(source, palette);
    } catch (error) {
      if (error instanceof Refused) {
        // The page may still be drawing it: the copy then holds no error.
        figure.replaceChildren(broken(figure.ownerDocument, error.message, source));
        figure.removeAttribute('data-wide');
        continue;
      }
      throw error;
    }
    const box = figure.ownerDocument.createElement('div');
    box.className = 'diagram-svg';
    box.innerHTML = svg;
    figure.replaceChildren(box);
    figure.setAttribute('data-scheme', 'light');
    if ((naturalWidth(svg) ?? 0) > WIDE) {
      figure.setAttribute('data-wide', 'true');
    } else {
      figure.removeAttribute('data-wide');
    }
  }
}

/** The error of a refused diagram, as DiagramView shows it in the cheatsheet. */
function broken(doc: Document, message: string, source: string): HTMLElement {
  const box = doc.createElement('div');
  box.className = 'diagram-broken';
  const head = doc.createElement('p');
  head.className = 'diagram-broken-head';
  head.textContent = 'Mermaid cannot draw this diagram.';
  const error = doc.createElement('pre');
  error.className = 'diagram-error';
  error.textContent = message;
  const shown = doc.createElement('pre');
  shown.className = 'diagram-source';
  const code = doc.createElement('code');
  code.textContent = source;
  shown.append(code);
  box.append(head, error, shown);
  return box;
}

/**
 * Put the bytes of each image in the file, as a data: URI, until the
 * budget is spent. An image that does not fit, or that the server does
 * not send, leaves its alt text in its place.
 */
async function embed(blocks: [Block, Element][], sources: Sources) {
  let left = MEDIA_BUDGET;
  for (const [block, element] of blocks) {
    const body = element.querySelector('.image-body');
    if (block.kind !== 'image' || !body) {
      continue;
    }
    const doc = body.ownerDocument;
    const bytes = await sources.media(block.media).catch(() => null);
    if (bytes && bytes.size <= left) {
      left -= bytes.size;
      const image = doc.createElement('img');
      image.src = await dataUri(bytes, block);
      image.alt = block.alt;
      if (block.width !== null && block.height !== null) {
        image.width = block.width;
        image.height = block.height;
      }
      body.replaceChildren(image);
    } else {
      const note = doc.createElement('p');
      note.className = 'image-left-out';
      note.textContent = bytes
        ? `Image left out of this file, which holds ${MEDIA_BUDGET / 1024 / 1024} MB of images at most: ${block.alt}`
        : `Image that the server did not send: ${block.alt}`;
      body.replaceChildren(note);
    }
  }
}

async function dataUri(bytes: Blob, image: ImageBlock): Promise<string> {
  const all = new Uint8Array(await bytes.arrayBuffer());
  let text = '';
  // In chunks: a spread of millions of arguments overflows the stack.
  for (let at = 0; at < all.length; at += 0x8000) {
    text += String.fromCharCode(...all.subarray(at, at + 0x8000));
  }
  // The type that the server read in the bytes, never the one of the answer.
  return `data:${image.type};base64,${btoa(text)}`;
}

/**
 * The custom properties that hold with no media condition: the tokens of
 * the light theme. A rule inside @media, such as the dark theme, is left
 * out. A rule inside @layer counts.
 */
export function lightTokens(sheets: Iterable<CSSStyleSheet>): Map<string, string> {
  const tokens = new Map<string, string>();
  const walk = (rules: Iterable<CSSRule>) => {
    for (const rule of rules) {
      if (isStyle(rule)) {
        if (rule.selectorText.split(',').some((part) => part.trim() === ':root')) {
          for (const name of Array.from(rule.style)) {
            if (name.startsWith('--')) {
              tokens.set(name, rule.style.getPropertyValue(name).trim());
            }
          }
        }
      } else if (isGroup(rule) && prelude(rule).startsWith('@layer')) {
        walk(rule.cssRules);
      }
    }
  };
  for (const sheet of sheets) {
    walk(rulesOf(sheet));
  }
  return tokens;
}

const VAR = /var\(\s*(--[\w-]+)\s*(?:,\s*([^()]*))?\)/g;

/**
 * Write the value of each token in the inline styles, such as the colors
 * of the series of a chart: the SVG then holds its colors, as Mermaid's
 * does.
 */
function resolve(copy: Element, tokens: Map<string, string>) {
  for (const element of [copy, ...copy.querySelectorAll('[style]')]) {
    const style = element.getAttribute('style');
    if (style?.includes('var(')) {
      element.setAttribute(
        'style',
        style.replace(VAR, (whole, name: string, fallback?: string) => {
          return tokens.get(name) ?? fallback?.trim() ?? whole;
        }),
      );
    }
  }
}

/** What the page needs to play or to open, and nothing of the file uses. */
const LEFT_OUT = [
  'script',
  'button',
  '.ask',
  '.threads',
  '.chart-hit',
  '.chart-tip',
  '.chart-band',
  '.chart-cross',
].join(', ');

/**
 * Take out what the file cannot use: the buttons, the targets of the
 * pointer, the comments of the templates, and every link, since a page of
 * the course or of the server does not exist outside the app and the file
 * points nowhere. The text of a link stays. Then no attribute runs or
 * loads anything.
 */
function sweep(copy: Element) {
  // An image of the file holds its bytes: any other one would load.
  for (const element of copy.querySelectorAll(`${LEFT_OUT}, img:not([src^="data:"])`)) {
    element.remove();
  }
  const doc = copy.ownerDocument;
  const comments = doc.createTreeWalker(copy, NodeFilter.SHOW_COMMENT);
  const found: Node[] = [];
  for (let node = comments.nextNode(); node; node = comments.nextNode()) {
    found.push(node);
  }
  for (const node of found) {
    node.parentNode?.removeChild(node);
  }
  for (const link of copy.querySelectorAll('a')) {
    const text = doc.createElement('span');
    const kept = link.getAttribute('class');
    if (kept) {
      text.setAttribute('class', kept);
    }
    text.append(...link.childNodes);
    link.replaceWith(text);
  }
  for (const element of [copy, ...copy.querySelectorAll('*')]) {
    for (const { name } of Array.from(element.attributes)) {
      // xmlns names the SVG namespace by an http URL, and HTML needs none.
      if (
        /^on/i.test(name) ||
        name.startsWith('data-v-') ||
        name === 'tabindex' ||
        name === 'xmlns' ||
        name.startsWith('xmlns:')
      ) {
        element.removeAttribute(name);
      }
    }
  }
}

/**
 * The rules of the page that the file needs: a style rule that matches an
 * element of the file, inside the @media, @supports and @layer blocks
 * that hold one, and the @page rule. A dark theme never comes along.
 */
export function css(sheets: Iterable<CSSStyleSheet>, doc: Document): string {
  const keep = (rules: Iterable<CSSRule>): string[] => {
    const out: string[] = [];
    for (const rule of rules) {
      const text = rule.cssText;
      if (text.startsWith('@page')) {
        out.push(text);
      } else if (isStyle(rule)) {
        if (matches(rule.selectorText, doc)) {
          out.push(text);
        }
      } else if (isGroup(rule)) {
        const head = prelude(rule);
        if (
          !/^@(media|supports|layer|container)\b/.test(head) ||
          /color-scheme:\s*dark/.test(head)
        ) {
          continue;
        }
        const inner = keep(rule.cssRules);
        if (inner.length) {
          out.push(`${head} {\n${inner.join('\n')}\n}`);
        }
      } else if (text.startsWith('@layer')) {
        // The order of the layers, which sets their weight.
        out.push(text);
      }
    }
    return out;
  };
  return [...sheets].flatMap((sheet) => keep(rulesOf(sheet))).join('\n');
}

/** Whether a selector list matches an element of the document. */
function matches(selectors: string, doc: Document): boolean {
  // A pseudo-element matches no element: its element decides.
  const parts = split(selectors).map(
    (part) =>
      part
        .replace(/::[\w-]+(\([^)]*\))?/g, '')
        .replace(/:(before|after|first-line|first-letter)\b/g, '')
        .trim() || '*',
  );
  try {
    return doc.querySelector(parts.join(', ')) !== null;
  } catch {
    // A selector that this engine cannot test, such as :host: keep it.
    return true;
  }
}

/** A selector list, cut at the commas that no parenthesis holds. */
function split(selectors: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let start = 0;
  for (let at = 0; at < selectors.length; at += 1) {
    const char = selectors[at];
    if (char === '(') {
      depth += 1;
    } else if (char === ')') {
      depth -= 1;
    } else if (char === ',' && depth === 0) {
      out.push(selectors.slice(start, at));
      start = at + 1;
    }
  }
  out.push(selectors.slice(start));
  return out;
}

function rulesOf(sheet: CSSStyleSheet): Iterable<CSSRule> {
  try {
    return sheet.cssRules;
  } catch {
    // A sheet of another origin hides its rules. The page loads none.
    return [];
  }
}

function isStyle(rule: CSSRule): rule is CSSStyleRule {
  return 'selectorText' in rule && 'style' in rule;
}

function isGroup(rule: CSSRule): rule is CSSGroupingRule {
  return 'cssRules' in rule;
}

/** The text of a block rule before its brace, such as `@media print`. */
function prelude(rule: CSSRule): string {
  const text = rule.cssText;
  return text.slice(0, text.indexOf('{')).trim();
}
