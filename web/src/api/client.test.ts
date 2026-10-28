import { describe, expect, it } from 'vitest';
import { ApiError, serverGone } from './client';

describe('serverGone', () => {
  it('takes a request that reaches nothing for a stopped server', () => {
    expect(serverGone(new TypeError('Failed to fetch'))).toBe(true);
  });

  it('takes a refused token for another server on the port', () => {
    expect(serverGone(new ApiError('unauthorized', 'Unauthorized'))).toBe(true);
  });

  it('leaves an error of a route to the page', () => {
    expect(serverGone(new ApiError('notFound', 'page 3 has no content yet'))).toBe(false);
  });
});
