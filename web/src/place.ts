// Where the learner is, and how it rides in the URL.
//
// The browser owns the history of a course: back and forward walk the pages
// the learner read, and a reload comes back to the last one. The place goes
// in the fragment, because the query already carries the session token and
// the server serves one page under one address.

export interface Place {
  /** The id of the page being read. */
  page: string;
  /** The block whose thread is open, or WHOLE_PAGE, when one is. */
  block?: string;
}

/**
 * The block of the address when the learner asks about the whole page. A
 * block id is 12 hex digits, so it never takes this value.
 */
export const WHOLE_PAGE = 'page';

/**
 * The fragment of the cheatsheet. It holds no `page`, so `parse` finds no
 * place in it.
 */
export const CHEATSHEET = '#cheatsheet';

/** The fragment of a place, `#page=7a&block=3f9c2e81d0b4`. */
export function format(place: Place): string {
  const parts = [`page=${encodeURIComponent(place.page)}`];

  if (place.block) {
    parts.push(`block=${encodeURIComponent(place.block)}`);
  }
  return `#${parts.join('&')}`;
}

/** The place a fragment names, or `null` when it names none. */
export function parse(fragment: string): Place | null {
  const fields = new URLSearchParams(fragment.replace(/^#/, ''));
  const page = fields.get('page');
  if (!page) {
    return null;
  }

  return { page, block: fields.get('block') || undefined };
}
