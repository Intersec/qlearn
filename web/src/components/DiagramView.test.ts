// @vitest-environment jsdom
// Mermaid is mocked at the module boundary: the real one runs in the
// browser tests, which jsdom cannot stand in for.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { enableAutoUnmount, flushPromises, mount } from '@vue/test-utils';
import type { MermaidConfig } from 'mermaid';
import DiagramView from './DiagramView.vue';
import type { DiagramBlock } from '@/api/types';

const mermaid = vi.hoisted(() => ({
  initialize: vi.fn(),
  parse: vi.fn(),
  render: vi.fn(),
}));

vi.mock('mermaid', () => ({ default: mermaid }));

// A view left mounted would draw again on the theme changes of later tests.
enableAutoUnmount(afterEach);

const SVG =
  '<svg id="diagram-1" xmlns="http://www.w3.org/2000/svg"><g><text>Follower</text></g></svg>';

function block(source = 'graph LR\n  A --> B\n'): DiagramBlock {
  return { id: 'd1', kind: 'diagram', source };
}

/** The config of the last call to `initialize`. */
function config(): MermaidConfig {
  return mermaid.initialize.mock.lastCall?.[0] as MermaidConfig;
}

async function view(source?: string) {
  const wrapper = mount(DiagramView, { props: { block: block(source) } });
  await flushPromises();
  return wrapper;
}

beforeEach(() => {
  // Here, not after a test: the view of that test may still listen then.
  document.documentElement.removeAttribute('data-theme');
  document.documentElement.removeAttribute('style');
  mermaid.parse.mockResolvedValue({ diagramType: 'flowchart-v2' });
  mermaid.render.mockResolvedValue({ svg: SVG });
});

afterEach(() => {
  vi.resetAllMocks();
});

describe('DiagramView', () => {
  it('draws the SVG of Mermaid', async () => {
    const wrapper = await view();

    expect(mermaid.render).toHaveBeenCalledWith(expect.any(String), 'graph LR\n  A --> B\n');
    expect(wrapper.find('.diagram-svg svg text').text()).toBe('Follower');
    expect(wrapper.find('.diagram-broken').exists()).toBe(false);
    expect(wrapper.emitted('broken')).toBeUndefined();
  });

  it('runs Mermaid in its strict mode, quiet, with the colors of the page', async () => {
    document.documentElement.style.setProperty('--page', '#ffffff');
    document.documentElement.style.setProperty('--ink', '#202124');

    await view();

    expect(config()).toMatchObject({
      startOnLoad: false,
      securityLevel: 'strict',
      theme: 'base',
      htmlLabels: false,
      logLevel: 'fatal',
      suppressErrorRendering: true,
      themeVariables: { darkMode: false, background: '#ffffff', primaryTextColor: '#202124' },
    });
  });

  it('passes the SVG through DOMPurify', async () => {
    mermaid.render.mockResolvedValue({
      svg:
        '<svg xmlns="http://www.w3.org/2000/svg" onload="window.ran = true">' +
        '<script>window.ran = true</script><text>kept</text>' +
        '<foreignObject><div onclick="window.ran = true">html</div></foreignObject></svg>',
    });

    const wrapper = await view();

    const html = wrapper.find('.diagram-svg').html();
    expect(wrapper.find('.diagram-svg text').text()).toBe('kept');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('onload');
    expect(html).not.toContain('onclick');
    expect(html).not.toContain('foreignObject');
  });

  it('shows the error and the source of a refused diagram, and reports it', async () => {
    mermaid.parse.mockRejectedValue(new Error("Parse error on line 2:\nExpecting 'EOF'"));

    const wrapper = await view('graph LR\n  A -->\n');

    expect(wrapper.find('.diagram-svg').exists()).toBe(false);
    expect(wrapper.find('.diagram-error').text()).toBe("Parse error on line 2:\nExpecting 'EOF'");
    expect(wrapper.find('.diagram-source').text()).toBe('graph LR\n  A -->');
    expect(wrapper.find('.diagram-broken-head').text()).toContain('The agent gets the error');
    expect(wrapper.emitted('broken')).toEqual([['d1', "Parse error on line 2:\nExpecting 'EOF'"]]);
    expect(mermaid.render).not.toHaveBeenCalled();
  });

  it('reports a diagram that Mermaid parses but cannot render', async () => {
    mermaid.render.mockRejectedValue(new Error('Could not find a suitable point'));

    const wrapper = await view();

    expect(wrapper.find('.diagram-error').text()).toBe('Could not find a suitable point');
    expect(wrapper.emitted('broken')).toEqual([['d1', 'Could not find a suitable point']]);
  });

  it('does not send the agent an error of the page itself', async () => {
    mermaid.initialize.mockImplementation(() => {
      throw new Error('cannot read the theme');
    });

    const wrapper = await view();

    expect(wrapper.find('.diagram-error').text()).toBe('cannot read the theme');
    expect(wrapper.find('.diagram-broken-head').text()).toContain('Reload it');
    expect(wrapper.emitted('broken')).toBeUndefined();
  });

  it('draws again with the new colors when the theme changes', async () => {
    const wrapper = await view();
    expect(config().themeVariables.darkMode).toBe(false);

    document.documentElement.dataset.theme = 'dark';
    await vi.waitFor(() => expect(mermaid.render).toHaveBeenCalledTimes(2));
    await flushPromises();

    expect(config().themeVariables.darkMode).toBe(true);
    expect(wrapper.find('.diagram-svg svg').exists()).toBe(true);
  });

  it('draws again when the page is read again', async () => {
    const wrapper = await view();

    await wrapper.setProps({ block: block() });
    await flushPromises();

    expect(mermaid.render).toHaveBeenCalledTimes(2);
  });

  it('stops listening to the theme once gone', async () => {
    const wrapper = await view();
    wrapper.unmount();

    document.documentElement.dataset.theme = 'dark';
    await flushPromises();

    expect(mermaid.render).toHaveBeenCalledTimes(1);
  });
});
