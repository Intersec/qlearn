// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import CalloutView from './CalloutView.vue';
import type { Marker } from '@/api/types';

function view(marker: Marker, html = '<p>One term, one vote.</p>') {
  return mount(CalloutView, { props: { marker, html } });
}

describe('CalloutView', () => {
  it('draws each marker with its sign and its name', () => {
    const shown = (['hook', 'teaser', 'pitfall', 'note'] as const).map((marker) => {
      const wrapper = view(marker);
      return [
        wrapper.find('.callout').attributes('data-marker'),
        wrapper.find('.callout-sign').text(),
        wrapper.find('.callout-name').text(),
      ];
    });

    expect(shown).toEqual([
      ['hook', '🧠', 'Hook'],
      ['teaser', '🔭', 'Teaser'],
      ['pitfall', '⚠️', 'Pitfall'],
      ['note', 'ℹ️', 'Note'],
    ]);
  });

  it('shows the body that the server wrote', () => {
    const wrapper = view(
      'pitfall',
      '<p>Call <code>grant_vote()</code> once.</p><ul><li>a</li></ul>',
    );

    const body = wrapper.find('.callout-body');
    expect(body.find('code').text()).toBe('grant_vote()');
    expect(body.findAll('li').map((item) => item.text())).toEqual(['a']);
  });

  it('passes the HTML of the server through DOMPurify', () => {
    const wrapper = view(
      'hook',
      '<p>kept</p><script>window.ran = true</script><img src="x" onerror="window.ran = true">',
    );

    const html = wrapper.find('.callout-body').html();
    expect(html).toContain('<p>kept</p>');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('onerror');
  });

  it('keeps the sign out of what a screen reader says', () => {
    expect(view('hook').find('.callout-sign').attributes('aria-hidden')).toBe('true');
  });
});
