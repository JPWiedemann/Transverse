import { describe, expect, it } from 'vitest';
import { migrateLines } from './settings';

describe('saved settings', () => {
  it('are found under the new id of a renamed line type', () => {
    const old = { synthZ0: '42' };
    expect(migrateLines({ cpwg_inner: old })).toEqual({ planar: old });
    // what was already saved under the new id wins
    expect(migrateLines({ cpwg_inner: old, planar: { synthZ0: '35' } })).toEqual({ planar: { synthZ0: '35' } });
    expect(migrateLines({})).toEqual({});
  });
});
