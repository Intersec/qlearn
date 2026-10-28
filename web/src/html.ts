// The HTML that a page shows.
//
// The server writes every tag of a block itself, from a short list, and
// escapes the HTML of the source. This is the second lock: an agent writes
// the source, and nothing it writes may run in the page.

import DOMPurify from 'dompurify';

/** The HTML of a block, safe to give to v-html. */
export function clean(html: string): string {
  // `target` stays: a link to the web opens in a tab of its own, and the
  // server adds `rel="noopener noreferrer"` beside it.
  return DOMPurify.sanitize(html, { USE_PROFILES: { html: true }, ADD_ATTR: ['target'] });
}
