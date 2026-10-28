// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import PageView from './PageView.vue';
import type { Page } from '@/api/types';

function view(...html: string[]) {
  const page: Page = {
    id: '1',
    blocks: html.map((text, at) => ({ id: `block${at}`, kind: 'text', html: text })),
  };
  return mount(PageView, { props: { page } });
}

describe('PageView', () => {
  it('shows the blocks in order, each with its id', () => {
    const wrapper = view('<h1>Two leaders</h1>', '<h2>The vote</h2>');

    const blocks = wrapper.findAll('[data-block]');
    expect(blocks.map((block) => block.attributes('data-block'))).toEqual(['block0', 'block1']);
    expect(blocks.map((block) => block.text())).toEqual(['Two leaders', 'The vote']);
  });

  it('passes the HTML of the server through DOMPurify', () => {
    const wrapper = view(
      '<p>kept</p><script>window.ran = true</script><img src="x" onerror="window.ran = true">',
    );

    const html = wrapper.html();
    expect(html).toContain('<p>kept</p>');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('onerror');
  });

  it('keeps the colors of a fence', () => {
    const wrapper = view(
      '<pre class="md-code"><code><span class="tok-keyword tok-control">return</span></code></pre>',
    );

    expect(wrapper.find('.md-code .tok-keyword').text()).toBe('return');
  });
});
