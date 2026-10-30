// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import ImageView from './ImageView.vue';
import type { ImageBlock } from '@/api/types';

const COMMIT = '8571d1d281bb5ea58312280d207e4bbd40d1596b';
const MEDIA = '417270c99e2fbb74da534f6986f16b11a9a45914c3c79351a8a5203b82bd2285.svg';

function image(over: Partial<ImageBlock> = {}): ImageBlock {
  return {
    id: 'b093141c73c1',
    kind: 'image',
    alt: 'The states of a server',
    media: MEDIA,
    path: 'docs/states.svg',
    origin: 'repository',
    repo: '',
    commit: COMMIT,
    type: 'image/svg+xml',
    width: 120,
    height: 40,
    ...over,
  };
}

function view(value: ImageBlock = image()) {
  return mount(ImageView, { props: { image: value } });
}

describe('ImageView', () => {
  it('draws the copy of the store with its alt text and its size', () => {
    const img = view().find('img');

    expect(img.attributes('src')).toBe(`/api/media/${MEDIA}`);
    expect(img.attributes('alt')).toBe('The states of a server');
    expect(img.attributes('width')).toBe('120');
    expect(img.attributes('height')).toBe('40');
  });

  // An SVG in an <img> runs no script: the page never inlines it.
  it('draws an SVG from an img, never inline', () => {
    const wrapper = view();

    expect(wrapper.find('svg').exists()).toBe(false);
    expect(wrapper.find('img').exists()).toBe(true);
  });

  it('leaves the size out when the file does not say it', () => {
    const img = view(image({ width: null, height: null })).find('img');

    expect(img.attributes('width')).toBeUndefined();
    expect(img.attributes('height')).toBeUndefined();
  });

  it('says where the image comes from, with the short hash', () => {
    const wrapper = view();

    expect(wrapper.find('.image-path').text()).toBe('docs/states.svg');
    const commit = wrapper.find('.image-commit');
    expect(commit.text()).toBe('8571d1d');
    expect(commit.attributes('title')).toBe(COMMIT);
    expect(wrapper.find('.image-repo').exists()).toBe(false);
  });

  it('names the submodule that holds the file', () => {
    const wrapper = view(image({ path: 'vendor/lib/docs/logo.svg', repo: 'vendor/lib' }));

    expect(wrapper.find('.image-repo').text()).toBe('in vendor/lib');
  });

  it('shows the path of an image of the notes, with no commit', () => {
    const wrapper = view(
      image({ path: '.learn/crochet/notes/ring.svg', origin: 'notes', repo: null, commit: null }),
    );

    expect(wrapper.find('.image-path').text()).toBe('.learn/crochet/notes/ring.svg');
    expect(wrapper.find('.image-commit').exists()).toBe(false);
    expect(wrapper.find('.image-repo').exists()).toBe(false);
    expect(wrapper.find('img').attributes('src')).toBe(`/api/media/${MEDIA}`);
  });

  it('opens the full size in a tab of its own, with no opener', () => {
    const link = view().find('a');

    expect(link.attributes('href')).toBe(`/api/media/${MEDIA}`);
    expect(link.attributes('target')).toBe('_blank');
    expect(link.attributes('rel')).toBe('noopener noreferrer');
  });
});
