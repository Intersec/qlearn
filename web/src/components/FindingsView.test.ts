// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import FindingsView from './FindingsView.vue';
import type { Finding } from '@/api/types';

function finding(id: string, fields: Partial<Finding> = {}): Finding {
  return {
    id,
    title: `Finding ${id}`,
    body: `<p>The body of <code>${id}</code>.</p>`,
    checked: false,
    ...fields,
  };
}

describe('FindingsView', () => {
  it('shows nothing while the course holds no finding', () => {
    const wrapper = mount(FindingsView, { props: { findings: [] } });

    expect(wrapper.find('.findings').exists()).toBe(false);
  });

  it('counts the findings and those to check, and opens the list on demand', async () => {
    const wrapper = mount(FindingsView, {
      props: { findings: [finding('a'), finding('b', { checked: true }), finding('c')] },
    });

    const toggle = wrapper.get('.findings-toggle');
    expect(toggle.text()).toContain('Findings');
    expect(wrapper.get('.findings-count').text()).toBe('3');
    expect(wrapper.get('.findings-to-check').text()).toBe('2 to check');
    expect(toggle.attributes('aria-expanded')).toBe('false');
    expect(wrapper.find('.finding').exists()).toBe(false);

    await toggle.trigger('click');

    expect(toggle.attributes('aria-expanded')).toBe('true');
    expect(
      wrapper.findAll('[data-finding]').map((item) => item.attributes('data-finding')),
    ).toEqual(['a', 'b', 'c']);

    await toggle.trigger('click');
    expect(wrapper.find('.finding').exists()).toBe(false);
  });

  it('gives each finding its title, its body, and a badge', async () => {
    const wrapper = mount(FindingsView, {
      props: {
        findings: [
          finding('race', { title: 'A <race>' }),
          finding('doc', { checked: true, body: '' }),
        ],
      },
    });
    await wrapper.get('.findings-toggle').trigger('click');

    const race = wrapper.get('[data-finding="race"]');
    expect(race.attributes('data-checked')).toBe('false');
    expect(race.get('.finding-badge').text()).toBe('To check');
    expect(race.get('.finding-title').text()).toBe('A <race>');
    expect(race.get('.finding-body').html()).toContain('<code>race</code>');

    const doc = wrapper.get('[data-finding="doc"]');
    expect(doc.attributes('data-checked')).toBe('true');
    expect(doc.get('.finding-badge').text()).toBe('Checked');
    expect(doc.find('.finding-body').exists()).toBe(false);
  });

  it('says no count to check once every finding is checked', () => {
    const wrapper = mount(FindingsView, {
      props: { findings: [finding('a', { checked: true })] },
    });

    expect(wrapper.find('.findings-to-check').exists()).toBe(false);
  });

  it('cleans the HTML of a body before it shows', async () => {
    const wrapper = mount(FindingsView, {
      props: {
        findings: [finding('a', { body: '<p>Two</p><img src="x" onerror="window.ran = 1">' })],
      },
    });
    await wrapper.get('.findings-toggle').trigger('click');

    const body = wrapper.get('.finding-body').html();
    expect(body).toContain('<p>Two</p>');
    expect(body).not.toContain('onerror');
  });

  it('follows a list that the agent changed', async () => {
    const wrapper = mount(FindingsView, { props: { findings: [finding('a')] } });
    await wrapper.get('.findings-toggle').trigger('click');

    await wrapper.setProps({ findings: [finding('a', { checked: true }), finding('b')] });

    expect(wrapper.get('[data-finding="a"] .finding-badge').text()).toBe('Checked');
    expect(wrapper.findAll('.finding')).toHaveLength(2);
    expect(wrapper.get('.findings-to-check').text()).toBe('1 to check');
  });
});
