// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import ChoiceView from './ChoiceView.vue';
import type { ChosenAnswer, Choosing, ChoosingQuestion } from '@/api/types';

const NEXT: ChoosingQuestion = {
  header: 'Next step',
  prompt: 'Part 1 is done. What next?',
  options: [
    { label: 'Part 2', description: 'The code tour of the election' },
    { label: 'A break', description: '' },
  ],
  multi: false,
};

const TOPICS: ChoosingQuestion = {
  header: 'Topics',
  prompt: 'What should the follow-up cover?',
  options: [
    { label: 'The timer', description: '' },
    { label: 'The vote', description: '' },
    { label: 'The log', description: 'Not taught yet' },
  ],
  multi: true,
};

function choiceOf(questions: ChoosingQuestion[], answers: ChosenAnswer[] | null = null): Choosing {
  return { id: 'choice-3', page: 'c1', questions, answers };
}

/** A choice, with a send that takes every answer unless told otherwise. */
function view(choice: Choosing, took = true) {
  const send = vi.fn<(answers: ChosenAnswer[]) => Promise<boolean>>(async () => took);
  const wrapper = mount(ChoiceView, { props: { choice, send } });
  return { wrapper, send };
}

describe('ChoiceView', () => {
  it('shows a question that takes one pick as radio buttons, each with its description', () => {
    const { wrapper } = view(choiceOf([NEXT]));

    expect(wrapper.find('.choice').attributes('data-choice')).toBe('choice-3');
    expect(wrapper.find('.choice-head').text()).toBe('The agent asks');
    expect(wrapper.find('.choice-header').text()).toBe('Next step');
    expect(wrapper.find('.choice-prompt').text()).toBe('Part 1 is done. What next?');
    const radios = wrapper.findAll('input[type="radio"]');
    expect(radios.map((radio) => radio.attributes('name'))).toEqual(['choice-3-0', 'choice-3-0']);
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(0);
    expect(wrapper.findAll('.choice-label').map((label) => label.text())).toEqual([
      'Part 2',
      'A break',
    ]);
    // An option with no description shows none.
    expect(wrapper.findAll('.choice-description').map((text) => text.text())).toEqual([
      'The code tour of the election',
    ]);
    expect(wrapper.find('.choice-other-label').text()).toBe('Other');
  });

  it('sends the one pick of a question once it has one', async () => {
    const { wrapper, send } = view(choiceOf([NEXT]));
    const button = wrapper.find('.choice-send');
    expect(button.attributes('disabled')).toBeDefined();
    expect(wrapper.find('.choice-note').text()).toBe('Pick an option, or write your own.');

    await wrapper.findAll('input[type="radio"]')[1].setValue(true);
    expect(button.attributes('disabled')).toBeUndefined();
    await wrapper.find('form').trigger('submit');

    expect(send).toHaveBeenCalledWith([{ picked: [1], other: null }]);
  });

  it('sends several picks of a question that takes them, in order', async () => {
    const { wrapper, send } = view(choiceOf([NEXT, TOPICS]));
    const boxes = wrapper.findAll('input[type="checkbox"]');
    expect(boxes.map((box) => box.attributes('name'))).toEqual([
      'choice-3-1',
      'choice-3-1',
      'choice-3-1',
    ]);

    await wrapper.findAll('input[type="radio"]')[0].setValue(true);
    expect(wrapper.find('.choice-send').attributes('disabled')).toBeDefined();
    expect(wrapper.find('.choice-note').text()).toBe(
      'Answer each question: pick an option, or write your own.',
    );
    await boxes[2].setValue(true);
    await boxes[0].setValue(true);
    await wrapper.findAll('.choice-other-text')[1].setValue('  and the restart ');
    await wrapper.find('form').trigger('submit');

    expect(send).toHaveBeenCalledWith([
      { picked: [0], other: null },
      { picked: [0, 2], other: 'and the restart' },
    ]);
  });

  it('sends free text alone, with no pick', async () => {
    const { wrapper, send } = view(choiceOf([NEXT]));
    const other = wrapper.find('.choice-other-text');

    await other.setValue('   ');
    expect(wrapper.find('.choice-send').attributes('disabled')).toBeDefined();
    await other.setValue('Part 2, after lunch');
    expect(wrapper.find('.choice-send').attributes('disabled')).toBeUndefined();
    await wrapper.find('form').trigger('submit');

    expect(send).toHaveBeenCalledWith([{ picked: [], other: 'Part 2, after lunch' }]);
  });

  it('sends once, and keeps the picks when the server refuses them', async () => {
    let finish: (took: boolean) => void = () => {};
    const send = vi.fn(() => new Promise<boolean>((resolve) => (finish = resolve)));
    const wrapper = mount(ChoiceView, { props: { choice: choiceOf([NEXT]), send } });
    await wrapper.findAll('input[type="radio"]')[0].setValue(true);

    await wrapper.find('form').trigger('submit');
    await wrapper.find('form').trigger('submit');
    expect(send).toHaveBeenCalledTimes(1);
    expect(wrapper.find('.choice-send').attributes('disabled')).toBeDefined();

    finish(false);
    await flushPromises();
    const radio = wrapper.findAll('input[type="radio"]')[0].element as HTMLInputElement;
    expect(radio.checked).toBe(true);
    expect(wrapper.find('.choice-send').attributes('disabled')).toBeUndefined();
  });

  it('shows an answer read only, with the labels of the picks and the free text', () => {
    const { wrapper } = view(
      choiceOf(
        [NEXT, TOPICS],
        [
          { picked: [1], other: null },
          { picked: [0, 2], other: 'and the restart' },
        ],
      ),
    );

    expect(wrapper.find('.choice').attributes('data-answered')).toBe('true');
    expect(wrapper.find('.choice-head').text()).toBe('Your choice');
    expect(wrapper.find('form').exists()).toBe(false);
    expect(wrapper.findAll('input')).toHaveLength(0);
    const questions = wrapper.findAll('.choice-sent');
    expect(questions[0].findAll('.choice-pick').map((pick) => pick.text())).toEqual(['✓A break']);
    expect(questions[1].findAll('.choice-pick').map((pick) => pick.text())).toEqual([
      '✓The timer',
      '✓The log',
    ]);
    expect(questions[0].find('.choice-other-sent').exists()).toBe(false);
    expect(questions[1].find('.choice-other-sent').text()).toBe('Otherand the restart');
  });

  it('shows free text alone as the answer of a question', () => {
    const { wrapper } = view(choiceOf([NEXT], [{ picked: [], other: 'After lunch' }]));

    expect(wrapper.find('.choice-pick').exists()).toBe(false);
    expect(wrapper.find('.choice-other-sent').text()).toBe('OtherAfter lunch');
  });
});
