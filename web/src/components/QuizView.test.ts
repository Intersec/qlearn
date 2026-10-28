// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import QuizView from './QuizView.vue';
import type { Choice, GradedChoice, Quiz, QuizAnswer } from '@/api/types';

const CHOICES: Choice[] = [
  {
    header: 'Split vote',
    prompt: 'Two candidates get 2 votes each out of 5. What happens next?',
    options: ['The higher id wins', 'Both start a <em>new</em> election'],
    graded: null,
  },
  {
    header: 'Timer',
    prompt: 'Who starts the election?',
    options: ['The first to time out', 'The leader', 'Nobody'],
    graded: null,
  },
];

const OPEN = { prompt: 'Why is the timeout random?', sent: null };

/** The choices as the server answers them once graded. */
function graded(...grades: GradedChoice[]): Choice[] {
  return CHOICES.map((choice, at) => ({ ...choice, graded: grades[at] }));
}

/** A quiz, with a send that takes every part unless told otherwise. */
function view(quiz: Quiz, n = 1, took = true) {
  const send = vi.fn<(answer: QuizAnswer) => Promise<boolean>>(async () => took);
  const wrapper = mount(QuizView, { props: { quiz, n, send } });
  return { wrapper, send };
}

describe('QuizView', () => {
  it('shows the open question alone, with a text box', () => {
    const { wrapper } = view({ open: OPEN, choices: CHOICES }, 2);

    expect(wrapper.find('.quiz').attributes('data-quiz')).toBe('2');
    expect(wrapper.find('.quiz').attributes('data-step')).toBe('open');
    expect(wrapper.find('.quiz-head').text()).toBe('Quiz 2');
    expect(wrapper.find('.quiz-open label').text()).toBe('Why is the timeout random?');
    const box = wrapper.find('textarea');
    expect(box.attributes('id')).toBe(wrapper.find('.quiz-open label').attributes('for'));
    // The options of the choices would give the open answer away.
    expect(wrapper.find('.quiz-choices').exists()).toBe(false);
    expect(wrapper.text()).not.toContain('Split vote');
    expect(wrapper.text()).not.toContain('The higher id wins');
    expect(wrapper.find('.quiz-note').text()).toBe('The choices come after this answer.');
  });

  it('sends the open answer once there is one', async () => {
    const { wrapper, send } = view({ open: OPEN, choices: CHOICES });
    const button = wrapper.find('.quiz-send');
    expect(button.attributes('disabled')).toBeDefined();

    await wrapper.find('textarea').setValue('   ');
    expect(button.attributes('disabled')).toBeDefined();
    await wrapper.find('textarea').setValue('Two followers rarely wake up at once.');
    expect(button.attributes('disabled')).toBeUndefined();
    await wrapper.find('form').trigger('submit');

    expect(send).toHaveBeenCalledWith({ open: 'Two followers rarely wake up at once.' });
  });

  it('sends a part once, however often the learner presses Send', async () => {
    let finish: (took: boolean) => void = () => {};
    const send = vi.fn(() => new Promise<boolean>((resolve) => (finish = resolve)));
    const wrapper = mount(QuizView, { props: { quiz: { open: OPEN, choices: [] }, n: 1, send } });
    await wrapper.find('textarea').setValue('Because.');

    await wrapper.find('form').trigger('submit');
    await wrapper.find('form').trigger('submit');
    expect(wrapper.find('.quiz-send').attributes('disabled')).toBeDefined();
    expect(send).toHaveBeenCalledTimes(1);

    // The server refused it: the learner keeps the text and sends again.
    finish(false);
    await flushPromises();
    expect(wrapper.find('textarea').element.value).toBe('Because.');
    expect(wrapper.find('.quiz-send').attributes('disabled')).toBeUndefined();
  });

  it('shows the choices once the open answer is sent, each with an Other box', () => {
    const { wrapper } = view({
      open: { ...OPEN, sent: 'Two rarely wake at once.' },
      choices: CHOICES,
    });

    expect(wrapper.find('.quiz').attributes('data-step')).toBe('choices');
    expect(wrapper.find('.quiz-open').exists()).toBe(false);
    expect(wrapper.find('.quiz-sent .quiz-prompt').text()).toBe('Why is the timeout random?');
    expect(wrapper.find('.quiz-sent-text').text()).toBe('Two rarely wake at once.');
    const choices = wrapper.findAll('.quiz-choice');
    expect(choices.map((choice) => choice.find('.quiz-header').text())).toEqual([
      'Split vote',
      'Timer',
    ]);
    expect(choices.map((choice) => choice.find('.quiz-other-label').text())).toEqual([
      'Other',
      'Other',
    ]);
  });

  it('starts at the choices when there is no open question', () => {
    const { wrapper } = view({ open: null, choices: CHOICES });

    expect(wrapper.find('.quiz-open').exists()).toBe(false);
    expect(wrapper.find('.quiz-sent').exists()).toBe(false);
    const choices = wrapper.findAll('.quiz-choice');
    expect(choices[1].find('.quiz-prompt').text()).toBe('Who starts the election?');
    expect(choices[0].findAll('.quiz-option-text').map((option) => option.html())).toEqual([
      '<span class="md-doc quiz-option-text">The higher id wins</span>',
      '<span class="md-doc quiz-option-text">Both start a <em>new</em> election</span>',
    ]);
  });

  it('sends the choices once each has a pick, with the Other text', async () => {
    const { wrapper, send } = view({ open: null, choices: CHOICES }, 3);
    const radios = wrapper.findAll('input[type="radio"]');
    expect(radios.map((radio) => radio.attributes('name'))).toEqual([
      'quiz-3-0',
      'quiz-3-0',
      'quiz-3-1',
      'quiz-3-1',
      'quiz-3-1',
    ]);

    await radios[1].setValue(true);
    expect(wrapper.find('.quiz-send').attributes('disabled')).toBeDefined();
    expect(wrapper.find('.quiz-note').text()).toBe('Pick one option in each question.');
    await radios[4].setValue(true);
    expect(wrapper.find('.quiz-send').attributes('disabled')).toBeUndefined();
    expect(wrapper.find('.quiz-note').exists()).toBe(false);
    const others = wrapper.findAll('.quiz-other-text');
    await others[0].setValue('   ');
    await others[1].setValue('Unless no one times out');
    await wrapper.find('form').trigger('submit');

    expect(send).toHaveBeenCalledWith({
      choices: [
        { option: 1, other: null },
        { option: 2, other: 'Unless no one times out' },
      ],
    });
  });

  it('draws right and wrong at once, and says that the agent writes the debrief', () => {
    const { wrapper } = view({
      open: { ...OPEN, sent: 'Because.' },
      choices: graded(
        { picked: 1, answer: 1, ok: true, other: null },
        { picked: 2, answer: 0, ok: false, other: 'Unless no one times out' },
      ),
    });

    expect(wrapper.find('.quiz').attributes('data-step')).toBe('graded');
    expect(wrapper.find('form').exists()).toBe(false);
    expect(wrapper.find('input').exists()).toBe(false);
    const choices = wrapper.findAll('.quiz-graded');
    expect(choices.map((choice) => choice.attributes('data-ok'))).toEqual(['true', 'false']);
    expect(choices.map((choice) => choice.find('.quiz-verdict').text())).toEqual([
      'Right',
      'Wrong',
    ]);
    const marks = (at: number) =>
      choices[at].findAll('.quiz-option').map((option) => option.attributes('data-mark'));
    expect(marks(0)).toEqual(['none', 'right']);
    expect(marks(1)).toEqual(['right', 'none', 'wrong']);
    const second = choices[1].findAll('.quiz-option');
    expect(second[2].find('.quiz-tag').text()).toBe('Your pick');
    expect(second[2].find('.sr-only').text()).toBe('(your pick)');
    expect(second[0].find('.sr-only').text()).toBe('(the right option)');
    expect(choices[0].findAll('.quiz-option')[1].find('.sr-only').text()).toBe(
      '(your pick, the right option)',
    );
    expect(choices[0].find('.quiz-other-sent').exists()).toBe(false);
    expect(choices[1].findAll('.quiz-other-sent span').map((part) => part.text())).toEqual([
      'Other',
      'Unless no one times out',
    ]);
    expect(wrapper.find('.quiz-debrief').text()).toBe('The agent writes the debrief.');
  });

  it('sends an exercise at its open answer, and then waits for the debrief', () => {
    const { wrapper } = view({ open: OPEN, choices: [] });
    expect(wrapper.find('.quiz-note').exists()).toBe(false);

    const sent = view({ open: { ...OPEN, sent: 'Make the base random too.' }, choices: [] });
    expect(sent.wrapper.find('.quiz').attributes('data-step')).toBe('graded');
    expect(sent.wrapper.find('.quiz-sent-text').text()).toBe('Make the base random too.');
    expect(sent.wrapper.find('.quiz-debrief').exists()).toBe(true);
  });

  it('passes the HTML of an option through DOMPurify', () => {
    const options = ['<code>a</code><script>window.ran = true</script>', '<img src=x onerror=y>'];
    for (const choice of [
      { header: 'Code', prompt: 'Which?', options, graded: null },
      {
        header: 'Code',
        prompt: 'Which?',
        options,
        graded: { picked: 0, answer: 1, ok: false, other: null },
      },
    ]) {
      const { wrapper } = view({ open: null, choices: [choice] });

      const html = wrapper.find('.quiz-choice').html();
      expect(html).toContain('<code>a</code>');
      expect(html).not.toContain('<script');
      expect(html).not.toContain('onerror');
    }
  });
});
