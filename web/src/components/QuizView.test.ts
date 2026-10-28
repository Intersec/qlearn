// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import QuizView from './QuizView.vue';
import type { Quiz } from '@/api/types';

const CHOICES: Quiz['choices'] = [
  {
    header: 'Split vote',
    prompt: 'Two candidates get 2 votes each out of 5. What happens next?',
    options: ['The higher id wins', 'Both start a <em>new</em> election'],
  },
  {
    header: 'Timer',
    prompt: 'Who starts the election?',
    options: ['The first to time out', 'The leader', 'Nobody'],
  },
];

function view(quiz: Quiz, n = 1) {
  return mount(QuizView, { props: { quiz, n } });
}

describe('QuizView', () => {
  it('shows the open question alone, with a text box', () => {
    const wrapper = view({ open: { prompt: 'Why is the timeout random?' }, choices: CHOICES }, 2);

    expect(wrapper.find('.quiz').attributes('data-quiz')).toBe('2');
    expect(wrapper.find('.quiz-head').text()).toBe('Quiz 2');
    expect(wrapper.find('.quiz-open label').text()).toBe('Why is the timeout random?');
    const box = wrapper.find('textarea');
    expect(box.attributes('id')).toBe(wrapper.find('.quiz-open label').attributes('for'));
    // The options of the choices would give the open answer away.
    expect(wrapper.find('.quiz-choices').exists()).toBe(false);
    expect(wrapper.text()).not.toContain('Split vote');
    expect(wrapper.text()).not.toContain('The higher id wins');
  });

  it('keeps Send disabled, and says why', async () => {
    const wrapper = view({ open: { prompt: 'Why?' }, choices: [] });

    await wrapper.find('textarea').setValue('Two followers rarely wake up at once.');

    expect(wrapper.find('.quiz-send').attributes('disabled')).toBeDefined();
    expect(wrapper.find('.quiz-note').text()).toContain('cannot send an answer yet');
  });

  it('starts at the choices when there is no open question', () => {
    const wrapper = view({ open: null, choices: CHOICES });

    expect(wrapper.find('.quiz-open').exists()).toBe(false);
    const choices = wrapper.findAll('.quiz-choice');
    expect(choices.map((choice) => choice.find('.quiz-header').text())).toEqual([
      'Split vote',
      'Timer',
    ]);
    expect(choices[1].find('.quiz-prompt').text()).toBe('Who starts the election?');
    expect(choices[0].findAll('.quiz-option-text').map((option) => option.html())).toEqual([
      '<span class="md-doc quiz-option-text">The higher id wins</span>',
      '<span class="md-doc quiz-option-text">Both start a <em>new</em> election</span>',
    ]);
    expect(wrapper.find('.quiz-send').attributes('disabled')).toBeDefined();
  });

  it('gives each choice a group of its own', async () => {
    const wrapper = view({ open: null, choices: CHOICES }, 3);

    const radios = wrapper.findAll('input[type="radio"]');
    expect(radios.map((radio) => radio.attributes('name'))).toEqual([
      'quiz-3-0',
      'quiz-3-0',
      'quiz-3-1',
      'quiz-3-1',
      'quiz-3-1',
    ]);
    await radios[1].setValue(true);
    await radios[4].setValue(true);
    expect(radios.map((radio) => (radio.element as HTMLInputElement).checked)).toEqual([
      false,
      true,
      false,
      false,
      true,
    ]);
  });

  it('passes the HTML of an option through DOMPurify', () => {
    const wrapper = view({
      open: null,
      choices: [
        {
          header: 'Code',
          prompt: 'Which?',
          options: ['<code>a</code><script>window.ran = true</script>', '<img src=x onerror=y>'],
        },
      ],
    });

    const html = wrapper.find('.quiz-choice').html();
    expect(html).toContain('<code>a</code>');
    expect(html).not.toContain('<script');
    expect(html).not.toContain('onerror');
  });
});
