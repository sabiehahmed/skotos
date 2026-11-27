import crypto from 'node:crypto';
import { expect, test } from '@playwright/test';
import {
  ALL_CLAUSE_OPS,
  buildBucketHashInput,
  buildClause,
  buildFlagPatchRequest,
  buildRollout,
  computeBucketFromHashHex,
  computeSha1Bucket,
  evaluateRolloutForContext,
  findContextKeyForBucketRange,
  multiContext,
  parseAttributeRef,
  patchInstructions,
  resolveContextAttribute,
  selectRolloutVariation,
  singleContext,
  validateRolloutWeights,
} from '../../src/eval-helpers';
import {
  CLAUSE_OPS,
  FLAG_PATCH_INSTRUCTION_KINDS,
  ROLLOUT_TOTAL_WEIGHT,
  type FlagPatchInstruction,
  type WeightedVariation,
} from '../../src/types';

const VAR_A = '00000000-0000-4000-8000-00000000000a';
const VAR_B = '00000000-0000-4000-8000-00000000000b';
const VAR_C = '00000000-0000-4000-8000-00000000000c';

test.describe('Evaluation Helpers (e2e/src/eval-helpers.ts)', () => {
  test('matches all 10 verified SHA-1 60-bit reference vectors bit-for-bit including bucket 0 and 99999', () => {
    const referenceVectors: Array<{
      label: string;
      flagKey: string;
      salt: string;
      bucketValue: string;
      seed: number | null;
      secondary: string | null;
      expectedInput: string;
      expectedSha1: string;
      expectedH: bigint;
      expectedBucket: number;
    }> = [
      {
        label: '#1 standard key',
        flagKey: 'checkout-v2',
        salt: 'salt-12345',
        bucketValue: 'user-ada',
        seed: null,
        secondary: null,
        expectedInput: 'checkout-v2.salt-12345.user-ada',
        expectedSha1: '64106e739b659439d0c019d5428db6432a47e80c',
        expectedH: 450649027889224003n,
        expectedBucket: 39087,
      },
      {
        label: '#2 with _meta.secondary',
        flagKey: 'checkout-v2',
        salt: 'salt-12345',
        bucketValue: 'user-ada',
        seed: null,
        secondary: 'sec-99',
        expectedInput: 'checkout-v2.salt-12345.user-ada.sec-99',
        expectedSha1: 'b31329ea307ece86515414a3841c4d6680bd7e43',
        expectedH: 806481465197456616n,
        expectedBucket: 69951,
      },
      {
        label: '#3 empty secondary ignored',
        flagKey: 'checkout-v2',
        salt: 'salt-12345',
        bucketValue: 'user-ada',
        seed: null,
        secondary: '',
        expectedInput: 'checkout-v2.salt-12345.user-ada',
        expectedSha1: '64106e739b659439d0c019d5428db6432a47e80c',
        expectedH: 450649027889224003n,
        expectedBucket: 39087,
      },
      {
        label: '#4 custom seed overrides flag/salt/secondary',
        flagKey: 'checkout-v2',
        salt: 'salt-12345',
        bucketValue: 'user-ada',
        seed: 61,
        secondary: 'sec-99',
        expectedInput: '61.user-ada',
        expectedSha1: '4dd31ca5f068bf58d4d7329c2fbccda80e18c9dc',
        expectedH: 350491091252186101n,
        expectedBucket: 30400,
      },
      {
        label: '#5 numeric integer bucketBy',
        flagKey: 'pricing-tiers',
        salt: 'abc123',
        bucketValue: '42',
        seed: null,
        secondary: null,
        expectedInput: 'pricing-tiers.abc123.42',
        expectedSha1: '257a528354d64f2eadba6be46ae9efcbb0a29401',
        expectedH: 168785103161222386n,
        expectedBucket: 14639,
      },
      {
        label: '#6 UTF-8 Unicode key',
        flagKey: 'i18n-flag',
        salt: 's-utf8',
        bucketValue: 'ünicøde-ユーザー',
        seed: null,
        secondary: null,
        expectedInput: 'i18n-flag.s-utf8.ünicøde-ユーザー',
        expectedSha1: '0dd2581cad56e1671b8c17de2aa2d7fa2846ae26',
        expectedH: 62247209237048854n,
        expectedBucket: 5399,
      },
      {
        label: '#7 classic parity vector A',
        flagKey: 'hashKey',
        salt: 'saltyA',
        bucketValue: 'userKeyA',
        seed: null,
        secondary: null,
        expectedInput: 'hashKey.saltyA.userKeyA',
        expectedSha1: '6bec658111d6ad9dfa4ab424c3b8f32a5260c76d',
        expectedH: 486043891349154521n,
        expectedBucket: 42157,
      },
      {
        label: '#8 classic parity vector B',
        flagKey: 'hashKey',
        salt: 'saltyA',
        bucketValue: 'userKeyB',
        seed: null,
        secondary: null,
        expectedInput: 'hashKey.saltyA.userKeyB',
        expectedSha1: 'abbcba2157240ad79c9d4c7358fbed16cc5e7f77',
        expectedH: 773435658029121709n,
        expectedBucket: 67084,
      },
      {
        label: '#9 real SHA-1 lower boundary bucket=0',
        flagKey: 'rollout-edge',
        salt: 'edge-salt',
        bucketValue: 'boundary-255595',
        seed: null,
        secondary: null,
        expectedInput: 'rollout-edge.edge-salt.boundary-255595',
        expectedSha1: '0000087d354e2389499d1198ee9d81a184087a1a',
        expectedH: 583366140472n,
        expectedBucket: 0,
      },
      {
        label: '#10 real SHA-1 upper boundary bucket=99999',
        flagKey: 'rollout-edge',
        salt: 'edge-salt',
        bucketValue: 'boundary-163642',
        seed: null,
        secondary: null,
        expectedInput: 'rollout-edge.edge-salt.boundary-163642',
        expectedSha1: 'ffff7d591a15d1eeaa0430658499aa268ce4561a',
        expectedH: 1152912526273502494n,
        expectedBucket: 99999,
      },
    ];

    expect(referenceVectors).toHaveLength(10);

    for (const vec of referenceVectors) {
      const result = computeSha1Bucket({
        flagKey: vec.flagKey,
        salt: vec.salt,
        bucketValue: vec.bucketValue,
        seed: vec.seed,
        secondary: vec.secondary,
      });

      expect(result.input, vec.label).toBe(vec.expectedInput);
      expect(result.sha1Hex, vec.label).toBe(vec.expectedSha1);
      expect(result.h, vec.label).toBe(vec.expectedH);
      expect(result.bucket, vec.label).toBe(vec.expectedBucket);

      // Independent cross-verification against Buffer.readBigUInt64BE(0) >> 4n
      const digestBuf = crypto.createHash('sha1').update(vec.expectedInput, 'utf8').digest();
      const shiftedH = digestBuf.readBigUInt64BE(0) >> 4n;
      expect(result.h, `${vec.label} shiftedH`).toBe(shiftedH);
      expect(result.bucket, `${vec.label} basisPoints`).toBe(
        Number((shiftedH * 100_000n) / (1n << 60n)),
      );
    }
  });

  test('verifies 60-bit extremum bounds, determinism, and 50/50 distribution sanity', () => {
    const min = computeBucketFromHashHex('0000000000000000000000000000000000000000');
    expect(min.h).toBe(0n);
    expect(min.bucket).toBe(0);

    const max = computeBucketFromHashHex('ffffffffffffffffffffffffffffffffffffffff');
    expect(max.h).toBe((1n << 60n) - 1n);
    expect(max.bucket).toBe(99_999);

    expect(() => computeBucketFromHashHex('too-short')).toThrow(/Invalid hex hash/);

    // Determinism across 50 repeats
    const first = computeSha1Bucket({
      flagKey: 'checkout-v2',
      salt: 'salt-42',
      bucketValue: 'usr_123',
    }).bucket;
    for (let i = 0; i < 50; i++) {
      expect(
        computeSha1Bucket({
          flagKey: 'checkout-v2',
          salt: 'salt-42',
          bucketValue: 'usr_123',
        }).bucket,
      ).toBe(first);
    }

    // Distribution sanity across 2,000 keys on 50,000 / 50,000 split
    let bucketALow = 0;
    for (let i = 0; i < 2000; i++) {
      const { bucket } = computeSha1Bucket({
        flagKey: 'dist-check',
        salt: 'salt-uniform',
        bucketValue: `user-${i}`,
      });
      expect(Number.isInteger(bucket)).toBe(true);
      expect(bucket).toBeGreaterThanOrEqual(0);
      expect(bucket).toBeLessThan(ROLLOUT_TOTAL_WEIGHT);
      if (bucket < 50_000) bucketALow++;
    }
    expect(bucketALow).toBeGreaterThanOrEqual(900);
    expect(bucketALow).toBeLessThanOrEqual(1100);
  });

  test('seed overrides flagKey, salt, and _meta.secondary', () => {
    const a = buildBucketHashInput({
      flagKey: 'flag-one',
      salt: 'salt-one',
      bucketValue: 'user-7',
      seed: 9001,
      secondary: 'sec-a',
    });
    const b = buildBucketHashInput({
      flagKey: 'flag-two',
      salt: 'salt-two',
      bucketValue: 'user-7',
      seed: 9001,
      secondary: 'sec-b',
    });
    expect(a).toBe('9001.user-7');
    expect(b).toBe('9001.user-7');
  });

  test('cumulative rollout weights, boundary cases, fallback behavior, and experiment tracking', () => {
    const threeWay: WeightedVariation[] = [
      { variationId: VAR_A, weight: 25_000 },
      { variationId: VAR_B, weight: 50_000 },
      { variationId: VAR_C, weight: 25_000 },
    ];
    expect(selectRolloutVariation(39_087, threeWay).index).toBe(1);

    // bucket=0 skips 0-weight variation
    expect(
      selectRolloutVariation(0, [
        { variationId: VAR_A, weight: 0 },
        { variationId: VAR_B, weight: 100_000 },
      ]).index,
    ).toBe(1);
    expect(
      selectRolloutVariation(0, [
        { variationId: VAR_A, weight: 1 },
        { variationId: VAR_B, weight: 99_999 },
      ]).index,
    ).toBe(0);

    // bucket=99999 boundary
    expect(
      selectRolloutVariation(99_999, [
        { variationId: VAR_A, weight: 99_999 },
        { variationId: VAR_B, weight: 1 },
      ]).index,
    ).toBe(1);
    expect(
      selectRolloutVariation(99_999, [
        { variationId: VAR_A, weight: 100_000 },
        { variationId: VAR_B, weight: 0 },
      ]).index,
    ).toBe(0);

    // Under-allocated sum falls back to last variation
    expect(
      selectRolloutVariation(99_999, [
        { variationId: VAR_A, weight: 40_000 },
        { variationId: VAR_B, weight: 40_000 },
      ]).index,
    ).toBe(1);

    // Weight validation
    expect(validateRolloutWeights(threeWay)).toEqual({ valid: true, sum: 100_000, delta: 0 });
    expect(
      validateRolloutWeights([
        { variationId: VAR_A, weight: 60_000 },
        { variationId: VAR_B, weight: 39_999 },
      ]),
    ).toEqual({ valid: false, sum: 99_999, delta: -1 });

    // Missing contextKind and unhashable bucketBy -> bucket=0, usedFallback=true, inExperiment=false
    const expRollout = buildRollout(
      [
        { variationId: VAR_A, weight: 0 },
        { variationId: VAR_B, weight: 50_000, untracked: false },
        { variationId: VAR_C, weight: 50_000, untracked: true },
      ],
      { kind: 'experiment', contextKind: 'org', bucketBy: 'tier' },
    );

    const missingKind = evaluateRolloutForContext({
      context: singleContext('user-1', { tier: 'gold' }, { kind: 'user' }),
      rollout: expRollout,
      flagKey: 'f1',
      salt: 's1',
    });
    expect(missingKind).toEqual({
      bucket: 0,
      variationIndex: 1, // first weight > 0
      variationId: VAR_B,
      inExperiment: false,
      usedFallback: true,
    });

    for (const badVal of [42.5, true, null, { nested: 1 }] as const) {
      const unhashable = evaluateRolloutForContext({
        context: singleContext('org-1', { tier: badVal }, { kind: 'org' }),
        rollout: expRollout,
        flagKey: 'f1',
        salt: 's1',
      });
      expect(unhashable.usedFallback).toBe(true);
      expect(unhashable.bucket).toBe(0);
      expect(unhashable.inExperiment).toBe(false);
    }

    // Valid experiment evaluation on multi-context with integer bucketBy
    const validExp = evaluateRolloutForContext({
      context: multiContext({
        user: { key: 'u-1' },
        org: { key: 'o-1', attributes: { tier: 42 } },
      }),
      rollout: expRollout,
      flagKey: 'pricing-tiers',
      salt: 'abc123',
    });
    expect(validExp.usedFallback).toBe(false);
    expect(validExp.bucket).toBe(14_639);
    expect(validExp.variationId).toBe(VAR_B);
    expect(validExp.inExperiment).toBe(true);
  });

  test('RFC 6901 AttributeRef parsing & resolution blocks _meta and single-pass unescapes ~01 -> ~1', () => {
    expect(parseAttributeRef('')).toBeNull();
    expect(parseAttributeRef('/')).toBeNull();
    expect(parseAttributeRef('/address/')).toBeNull();
    expect(parseAttributeRef('/address//city')).toBeNull();
    expect(parseAttributeRef('/bad~2escape')).toBeNull();

    // Literal (no leading '/') does NOT unescape ~0 / ~1
    expect(parseAttributeRef('a~1b')).toEqual(['a~1b']);
    // Pointer (leading '/') single-pass unescapes ~1 -> '/' and ~0 -> '~'
    expect(parseAttributeRef('/address/country')).toEqual(['address', 'country']);
    expect(parseAttributeRef('/a~1b/c~0d/e~01f')).toEqual(['a/b', 'c~d', 'e~1f']);

    const ctx = singleContext(
      'usr-1',
      {
        address: { country: 'US' },
        'a/b': { 'c~d': 'found' },
        '~1': 'literal-tilde-one',
      },
      { kind: 'user', secondary: 'secret-secondary' },
    );

    expect(resolveContextAttribute(ctx, '/address/country')).toBe('US');
    expect(resolveContextAttribute(ctx, '/a~1b/c~0d')).toBe('found');
    expect(resolveContextAttribute(ctx, '/~01')).toBe('literal-tilde-one');
    expect(resolveContextAttribute(ctx, 'anonymous')).toBe(false);
    expect(resolveContextAttribute(ctx, 'kind')).toBe('user');
    expect(resolveContextAttribute(ctx, '_meta')).toBeUndefined();
    expect(resolveContextAttribute(ctx, '/_meta/secondary')).toBeUndefined();
  });

  test('singleContext, multiContext, buildClause, and findContextKeyForBucketRange', () => {
    const single = singleContext(
      'usr-ada',
      { plan: 'enterprise' },
      { name: 'Ada', anonymous: false, privateAttributes: ['email'], secondary: 's1' },
    );
    expect(single).toEqual({
      kind: 'user',
      key: 'usr-ada',
      name: 'Ada',
      anonymous: false,
      plan: 'enterprise',
      _meta: { privateAttributes: ['email'], secondary: 's1' },
    });

    const multi = multiContext({
      user: { key: 'u-1', attributes: { country: 'US' } },
      organization: { key: 'org-9', attributes: { tier: 'pro' } },
    });
    expect(multi.kind).toBe('multi');
    expect(multi.user).toEqual({ key: 'u-1', country: 'US' });
    expect('kind' in (multi.user as Record<string, unknown>)).toBe(false);
    expect(multi.organization).toEqual({ key: 'org-9', tier: 'pro' });

    const clause = buildClause('/address/country', 'in', ['US', 'CA'], {
      contextKind: 'user',
      negate: false,
    });
    expect(clause).toEqual({
      attribute: '/address/country',
      op: 'in',
      values: ['US', 'CA'],
      contextKind: 'user',
      negate: false,
    });

    const found = findContextKeyForBucketRange({
      flagKey: 'rollout-search',
      salt: 'salt-1',
      minBucket: 0,
      maxBucket: 9_999,
      prefix: 'low',
    });
    expect(found.bucket).toBeGreaterThanOrEqual(0);
    expect(found.bucket).toBeLessThanOrEqual(9_999);
    expect(
      computeSha1Bucket({
        flagKey: 'rollout-search',
        salt: 'salt-1',
        bucketValue: found.key,
      }).bucket,
    ).toBe(found.bucket);
  });

  test('exports exact 15 canonical ClauseOps and excludes eq/neq/exists', () => {
    expect(ALL_CLAUSE_OPS).toBe(CLAUSE_OPS);
    expect(CLAUSE_OPS).toHaveLength(15);
    expect(new Set(CLAUSE_OPS).size).toBe(15);

    const opsSet = new Set<string>(CLAUSE_OPS);
    for (const forbidden of ['eq', 'neq', 'exists']) {
      expect(opsSet.has(forbidden)).toBe(false);
    }
  });

  test('provides builders for all 23 FlagPatchInstruction variants and clean patch envelopes', () => {
    expect(FLAG_PATCH_INSTRUCTION_KINDS).toHaveLength(23);
    expect(Object.keys(patchInstructions)).toHaveLength(23);

    const rollout = buildRollout([
      { variationId: VAR_A, weight: 50_000 },
      { variationId: VAR_B, weight: 50_000 },
    ]);

    const built: FlagPatchInstruction[] = [
      patchInstructions.turnFlagOn(),
      patchInstructions.turnFlagOff(),
      patchInstructions.updateOffVariation(VAR_B),
      patchInstructions.addTargets(VAR_A, ['u-1'], 'user'),
      patchInstructions.removeTargets(VAR_A, ['u-1'], 'user'),
      patchInstructions.addRule({
        ruleId: 'r1',
        description: 'US beta',
        clauses: [buildClause('country', 'in', ['US'])],
        variationId: VAR_A,
      }),
      patchInstructions.updateRule({
        ruleId: 'r1',
        description: 'US+CA beta',
        clauses: [buildClause('country', 'in', ['US', 'CA'])],
        variationId: VAR_A,
      }),
      patchInstructions.removeRule('r1'),
      patchInstructions.reorderRules(['r2', 'r1']),
      patchInstructions.updateFallthrough({ rollout }),
      patchInstructions.addPrerequisite('prereq-flag', VAR_A),
      patchInstructions.removePrerequisite('prereq-flag'),
      patchInstructions.addVariation('v3', { name: 'Third' }),
      patchInstructions.updateVariation(VAR_A, { name: 'Primary', value: true }),
      patchInstructions.removeVariation(VAR_C),
      patchInstructions.updateClientSideAvailability(true, false),
      patchInstructions.archiveFlag(),
      patchInstructions.restoreFlag(),
      patchInstructions.updateName('Checkout V2'),
      patchInstructions.updateDescription('Updated description'),
      patchInstructions.updateTags(['checkout', 'payments']),
      patchInstructions.updateMaintainer('00000000-0000-4000-8000-000000000099'),
      patchInstructions.updateTemporary(false),
    ];

    expect(built).toHaveLength(23);
    const kinds = built.map((item) => item.kind);
    expect(new Set(kinds)).toEqual(new Set(FLAG_PATCH_INSTRUCTION_KINDS));

    const envelope = buildFlagPatchRequest({
      environmentKey: 'prod',
      expectedVersion: 5,
      comment: 'Enable checkout',
      instructions: [patchInstructions.turnFlagOn()],
    });
    expect(envelope).toEqual({
      environmentKey: 'prod',
      expectedVersion: 5,
      comment: 'Enable checkout',
      instructions: [{ kind: 'turnFlagOn' }],
    });

    const minimalEnvelope = buildFlagPatchRequest({
      instructions: [patchInstructions.updateName('Renamed')],
    });
    expect(Object.keys(minimalEnvelope)).toEqual(['instructions']);
  });
});
