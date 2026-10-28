// @vitest-environment jsdom
// DOMPurify needs a DOM. The browser has one; a node test has to ask.
import { describe, expect, it } from 'vitest';
import { clean } from './html';

describe('clean', () => {
  it('keeps the classes of a colored fence', () => {
    const html =
      '<pre class="md-code"><code><span class="tok-storage tok-type">int</span> x</code></pre>';

    expect(clean(html)).toBe(html);
  });

  it('keeps a table and its alignment', () => {
    const html = '<table><thead><tr><th class="md-right">Median</th></tr></thead></table>';

    expect(clean(html)).toBe(html);
  });

  it('keeps a link that opens in a tab of its own', () => {
    const html = '<a href="https://raft.github.io" target="_blank" rel="noopener noreferrer">x</a>';

    expect(clean(html)).toBe(html);
  });

  it('strips a script', () => {
    const html = clean('<p>before</p><script>alert(1)</script><p>after</p>');

    expect(html).not.toContain('<script');
    expect(html).toContain('after');
  });

  it('strips an event handler and a javascript link', () => {
    const html = clean('<img src="x" onerror="alert(1)"><a href="javascript:alert(1)">x</a>');

    expect(html).not.toContain('onerror');
    expect(html).not.toContain('javascript:');
  });
});
