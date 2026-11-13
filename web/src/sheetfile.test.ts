// @vitest-environment jsdom
// The HTML file of the cheatsheet, built from the sheet that the page
// draws. Mermaid is mocked at the module boundary, as in DiagramView.test.ts:
// the real one runs in the browser tests.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils';
import CheatsheetView from './components/CheatsheetView.vue';
import type { Cheatsheet, Course } from '@/api/types';
import { Refused } from './diagram';
import { MEDIA_BUDGET, css, fileName, lightTokens, sheetFile, type Sources } from './sheetfile';

const mermaid = vi.hoisted(() => ({
  initialize: vi.fn(),
  parse: vi.fn(),
  render: vi.fn(),
}));

vi.mock('mermaid', () => ({ default: mermaid }));

enableAutoUnmount(afterEach);

const COMMIT = '5aba991acd07dbe98266c4898e6bc5950240d118';

const COURSE: Course = {
  version: 1,
  slug: 'raft',
  title: 'Raft <elections>',
  goal: '',
  profile: '',
  quizMode: 'mixed',
  status: 'finished',
  followUp: null,
  parts: [],
  findings: [],
  reading: null,
  created: '2026-01-01T00:00:00Z',
  updated: '2026-01-01T00:00:00Z',
};

/** A sheet with a block of each kind, and both sections of the server. */
const SHEET: Cheatsheet = {
  written: true,
  blocks: [
    { id: 'title', kind: 'text', html: '<h1>Raft in one page</h1>' },
    {
      id: 'prose',
      kind: 'text',
      html:
        '<p>Read <a href="https://raft.github.io/" target="_blank" rel="noopener noreferrer">' +
        'the paper</a> first.</p><img src="x" onerror="alert(1)"><script>alert(2)</script>',
    },
    { id: 'hook', kind: 'callout', marker: 'hook', html: '<p>The first to wake up wins.</p>' },
    {
      id: 'timer',
      kind: 'excerpt',
      excerpt: {
        path: 'src/election.c',
        repo: '',
        commit: COMMIT,
        from: 1,
        to: 3,
        lang: 'c',
        lines: [
          {
            text: 'int election_timeout(int base)',
            spans: [{ start: 0, end: 3, cls: 'tok-storage' }],
          },
          null,
          { text: '}', spans: [] },
        ],
        marked: [1],
      },
    },
    { id: 'states', kind: 'diagram', source: 'flowchart LR\n  F --> C\n' },
    {
      id: 'times',
      kind: 'chart',
      chart: {
        kind: 'bar',
        title: 'Election time',
        x: { label: 'Range', values: ['150-300', '150-151'] },
        y: { label: 'Time', unit: 'ms' },
        series: [
          { name: '5 servers', values: [283, 1520] },
          { name: '9 servers', values: [301, 1985] },
        ],
      },
    },
    {
      id: 'vote',
      kind: 'steps',
      steps: {
        title: 'A vote',
        columns: ['Server', 'Term'],
        steps: [
          { label: 'Step zero', html: 'S1 wakes.', rows: [['S1', '6']], diagram: null },
          { label: 'Step one', html: 'S2 votes.', rows: [['S1', '7']], diagram: null },
          { label: 'Step two', html: 'S3 joins.', rows: null, diagram: 'sequenceDiagram\n' },
        ],
      },
    },
    {
      id: 'pixels',
      kind: 'image',
      alt: 'Six blue pixels',
      media: `${'a'.repeat(64)}.png`,
      path: 'docs/pixels.png',
      origin: 'repository',
      repo: '',
      commit: COMMIT,
      type: 'image/png',
      width: 3,
      height: 2,
    },
    {
      id: 'ring',
      kind: 'image',
      alt: 'The magic ring',
      media: `${'c'.repeat(64)}.svg`,
      path: '.learn/crochet/notes/ring.svg',
      origin: 'notes',
      repo: null,
      commit: null,
      type: 'image/svg+xml',
      width: null,
      height: null,
    },
    {
      id: 'big',
      kind: 'image',
      alt: 'A huge figure',
      media: `${'b'.repeat(64)}.png`,
      path: 'docs/huge.png',
      origin: 'repository',
      repo: '',
      commit: COMMIT,
      type: 'image/png',
      width: null,
      height: null,
    },
  ],
  map: [
    {
      path: 'src/election.c',
      repo: '',
      commit: COMMIT,
      from: 1,
      to: 6,
      lessons: [{ id: '1', title: 'The timer' }],
    },
  ],
  revisit: [
    {
      point: 'split-vote',
      label: 'How a split vote ends',
      result: 'missed',
      lesson: '1',
      title: 'The timer',
    },
  ],
};

/** The rules of the page, as main.css writes them, cut down. */
const STYLE = `
:root { --ink: #202124; --page: #ffffff; --series-1: #2a78d6; --series-2: #eb6834; }
@media screen and (prefers-color-scheme: dark) {
  :root:not([data-theme='light']) { --ink: #e8eaed; --page: #131416; }
}
.cheatsheet-sheet { column-count: 2; }
.excerpt .tok-storage { color: var(--tok-keyword); }
.quiz { border: 1px solid red; }
.md-doc a { color: blue; }
.agent-pill::before { content: ''; }
@media print {
  .cheatsheet-sheet { column-count: 2; }
  .top-bar { display: none; }
}
`;

/** A wide drawing, with the namespace and the style that Mermaid writes. */
function svg(id: string): string {
  return (
    `<svg id="${id}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 821 83" ` +
    `style="max-width: 821px;"><style>#${id} .node rect{fill:#eef4fe;}</style>` +
    `<g class="node"><rect></rect><text>Follower</text></g></svg>`
  );
}

let drawn = 0;
const sources: Sources = {
  draw: vi.fn(async () => svg(`diagram-file-${(drawn += 1)}`)),
  media: vi.fn(async (name: string) =>
    name.startsWith('b')
      ? new Blob([new Uint8Array(MEDIA_BUDGET)], { type: 'image/png' })
      : new Blob([new Uint8Array([137, 80, 78, 71])], { type: 'image/png' }),
  ),
};

let style: HTMLStyleElement;

beforeEach(() => {
  style = document.createElement('style');
  style.textContent = STYLE;
  document.head.append(style);
  mermaid.parse.mockResolvedValue({ diagramType: 'flowchart-v2' });
  mermaid.render.mockImplementation(async (id: string) => ({ svg: svg(id) }));
});

afterEach(() => {
  style.remove();
  vi.clearAllMocks();
});

/** The file of the fixture sheet, built from the view that draws it. */
async function built(): Promise<string> {
  const wrapper = mount(CheatsheetView, {
    props: { cheatsheet: SHEET, course: COURSE },
    attachTo: document.body,
  });
  await flushPromises();
  const sheet = wrapper.get('.cheatsheet-sheet').element as HTMLElement;
  return sheetFile(sheet, SHEET, `Cheatsheet: ${COURSE.title}`, sources);
}

function parsed(html: string): Document {
  return new DOMParser().parseFromString(html, 'text/html');
}

describe('sheetFile', () => {
  it('runs nothing, loads nothing, and points nowhere', async () => {
    const html = await built();

    expect(html).toMatch(/^<!doctype html>\n<html lang="en">/);
    expect(html).not.toMatch(/<script/i);
    expect(html).not.toMatch(/\son\w+=/i);
    expect(html).not.toMatch(/https?:/i);
    expect(html).not.toContain('/api/');
    expect(html).not.toContain('<a ');
    expect(html).not.toContain('<button');
    expect(html).not.toContain('<!--');
    expect(html).not.toContain('src="x"');
    // The text of a link stays.
    expect(parsed(html).body.textContent).toContain('Read the paper first.');
  });

  it('holds the head of a page of its own', async () => {
    const doc = parsed(await built());

    expect(doc.title).toBe('Cheatsheet: Raft <elections>');
    expect(doc.querySelector('meta[charset]')?.getAttribute('charset')).toBe('utf-8');
    expect(doc.querySelector('meta[name="viewport"]')).not.toBeNull();
    expect(doc.querySelector('meta[name="color-scheme"]')?.getAttribute('content')).toBe('light');
    expect(doc.querySelectorAll('style')).not.toHaveLength(0);
  });

  it('holds every block, and both sections of the server', async () => {
    const doc = parsed(await built());
    const sheet = doc.querySelector('article.cheatsheet > .cheatsheet-sheet')!;

    expect(sheet.querySelector('h1')?.textContent).toBe('Raft in one page');
    expect(sheet.querySelector('.callout[data-marker="hook"]')).not.toBeNull();

    const excerpt = sheet.querySelector('.excerpt')!;
    expect(excerpt.querySelector('.excerpt-path')?.textContent).toBe('src/election.c:1-3');
    expect(excerpt.querySelector('.excerpt-commit')?.textContent).toBe('5aba991');
    expect(excerpt.querySelector('.tok-storage')?.textContent).toBe('int');
    expect(excerpt.querySelector('.excerpt-marked')?.getAttribute('data-line')).toBe('1');
    // The hidden line stays a blank.
    expect(excerpt.querySelector('[data-line="2"] .excerpt-hidden')?.textContent).toBe('');

    expect(sheet.querySelector('.chart svg .chart-bar')).not.toBeNull();
    expect(sheet.querySelector('.chart figcaption')?.textContent).toBe('Election time');

    const steps = [...sheet.querySelectorAll('.steps-step .steps-label')].map((label) =>
      label.textContent?.replace(/\s+/g, ' ').trim(),
    );
    expect(steps).toEqual(['1 / 3 Step zero', '2 / 3 Step one', '3 / 3 Step two']);
    expect(
      sheet.querySelector('.steps-step[data-step="1"] td.steps-changed')?.textContent,
    ).toContain('7');

    expect(sheet.querySelector('#cheatsheet-map')?.textContent).toBe('Map of the code');
    expect(sheet.querySelector('.cheatsheet-where')?.textContent).toBe('src/election.c:1-6');
    expect(sheet.querySelector('#cheatsheet-revisit')?.textContent).toBe('To revisit');
    expect(sheet.querySelector('.cheatsheet-point .cheatsheet-label')?.textContent).toBe(
      'How a split vote ends',
    );
  });

  it('draws each diagram again, light, with an id of its own', async () => {
    const doc = parsed(await built());

    // The diagram block, and the diagram of the third step.
    expect(sources.draw).toHaveBeenCalledTimes(2);
    expect(
      vi.mocked(sources.draw).mock.calls.map(([source, palette]) => [source, palette.dark]),
    ).toEqual([
      ['flowchart LR\n  F --> C\n', false],
      ['sequenceDiagram\n', false],
    ]);
    // The palette is the light one, whatever the page shows.
    const palette = vi.mocked(sources.draw).mock.calls[0]![1];
    expect(palette.token('--ink')).toBe('#202124');

    const figures = [...doc.querySelectorAll('.diagram')];
    expect(figures).toHaveLength(2);
    const ids = figures.map((figure) => figure.querySelector('svg')?.id);
    expect(new Set(ids).size).toBe(2);
    for (const figure of figures) {
      expect(figure.getAttribute('data-scheme')).toBe('light');
      // Too wide for a column: it spans both, in the file too.
      expect(figure.getAttribute('data-wide')).toBe('true');
      // The style of Mermaid stays inside the SVG.
      expect(figure.querySelector('svg style')?.textContent).toContain('.node rect');
    }
  });

  it('shows the error of a refused diagram, even one the page still draws', async () => {
    // The page has not drawn yet when the learner saves.
    let release = () => {};
    const waiting = new Promise<void>((done) => {
      release = done;
    });
    mermaid.render.mockImplementation(async (id: string) => {
      await waiting;
      return { svg: svg(id) };
    });
    vi.mocked(sources.draw).mockImplementationOnce(async () => {
      throw new Refused("Parse error on line 2:\nExpecting 'EOF'");
    });

    try {
      const doc = parsed(await built());
      const figure = doc.querySelector('[data-block="states"] .diagram')!;

      expect(figure.querySelector('.diagram-drawing')).toBeNull();
      expect(figure.querySelector('.diagram-broken-head')?.textContent).toBe(
        'Mermaid cannot draw this diagram.',
      );
      expect(figure.querySelector('.diagram-error')?.textContent).toBe(
        "Parse error on line 2:\nExpecting 'EOF'",
      );
      expect(figure.querySelector('.diagram-source')?.textContent).toBe(
        'flowchart LR\n  F --> C\n',
      );
      expect(figure.hasAttribute('data-wide')).toBe(false);
    } finally {
      release();
      await flushPromises();
    }
  });

  it('writes the colors of a chart into its SVG', async () => {
    const doc = parsed(await built());
    const chart = doc.querySelector('.chart')!;

    expect(chart.innerHTML).not.toContain('var(--series');
    const fills = [...chart.querySelectorAll('.chart-bar')].map((bar) => bar.getAttribute('style'));
    expect(fills).toContain('fill: #2a78d6;');
    expect(fills).toContain('fill: #eb6834;');
    // What the pointer used, the file has no use for.
    expect(chart.querySelector('.chart-hit')).toBeNull();
  });

  it('puts the bytes of an image in the file, and leaves out one over the budget', async () => {
    const doc = parsed(await built());
    const images = [...doc.querySelectorAll('.image')];

    const small = images[0]!.querySelector('img')!;
    expect(small.getAttribute('src')).toBe('data:image/png;base64,iVBORw==');
    expect(small.getAttribute('alt')).toBe('Six blue pixels');
    expect(small.getAttribute('width')).toBe('3');
    // The caption still says where the image comes from.
    expect(images[0]!.querySelector('.image-path')?.textContent).toBe('docs/pixels.png');

    // An image of the notes is a file of the store like any other.
    const ring = images[1]!;
    expect(ring.querySelector('img')!.getAttribute('src')).toBe(
      'data:image/svg+xml;base64,iVBORw==',
    );
    expect(ring.querySelector('.image-path')?.textContent).toBe('.learn/crochet/notes/ring.svg');
    expect(ring.querySelector('.image-commit')).toBeNull();

    expect(images[2]!.querySelector('img')).toBeNull();
    expect(images[2]!.querySelector('.image-left-out')?.textContent).toBe(
      'Image left out of this file, which holds 8 MB of images at most: A huge figure',
    );
  });

  it('keeps the rules that the sheet needs, in light colors', async () => {
    const doc = parsed(await built());
    const text = [...doc.querySelectorAll('head style')].map((one) => one.textContent).join('\n');

    expect(text).toContain('--ink: #202124');
    expect(text).toContain('.excerpt .tok-storage');
    expect(text).toMatch(/@media print \{[^}]*\.cheatsheet-sheet \{ column-count: 2; \}/);
    // A rule with no element in the file, and the dark theme, stay out.
    expect(text).not.toContain('.quiz');
    expect(text).not.toContain('.top-bar');
    expect(text).not.toContain('.agent-pill');
    expect(text).not.toContain('.md-doc a');
    expect(text).not.toContain('#e8eaed');
  });

  it('names the file after the course', () => {
    expect(fileName('raft')).toBe('raft-cheatsheet.html');
  });
});

describe('lightTokens', () => {
  it('reads the tokens that hold with no media condition', () => {
    const tokens = lightTokens(document.styleSheets);

    expect(tokens.get('--ink')).toBe('#202124');
    expect(tokens.get('--series-2')).toBe('#eb6834');
  });
});

describe('css', () => {
  it('keeps a rule whose selector it cannot test', () => {
    const doc = document.implementation.createHTMLDocument('');
    const sheet = new CSSStyleSheet();
    sheet.insertRule('.never { color: red; }');
    sheet.insertRule(':root, :host { --x: 1px; }');

    expect(css([sheet], doc)).toContain('--x');
    expect(css([sheet], doc)).not.toContain('.never');
  });
});
