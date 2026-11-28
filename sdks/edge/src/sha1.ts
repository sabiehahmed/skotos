/**
 * Pure Web-Standard synchronous SHA-1 (FIPS 180-4) and FNV-1a 64-bit hashes.
 *
 * Zero Node.js imports (`node:crypto`) and zero async WebCrypto overhead so
 * Cloudflare Workers, Vercel Edge, Deno Deploy, and Fastly Compute evaluate
 * flags synchronously with 100% bit-exact parity against `crates/skotos-eval`.
 */

const textEncoder = new TextEncoder();

function rotl32(x: number, n: number): number {
  return ((x << n) | (x >>> (32 - n))) >>> 0;
}

/**
 * Computes lowercase 40-character hex SHA-1 digest of a UTF-8 string.
 */
export function sha1Hex(input: string): string {
  const bytes = textEncoder.encode(input);
  const bitLen = bytes.length * 8;

  // Pad to 56 mod 64 bytes, plus 8 bytes for 64-bit big-endian length.
  const paddedLen = Math.ceil((bytes.length + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLen);
  padded.set(bytes);
  padded[bytes.length] = 0x80;

  const view = new DataView(padded.buffer);
  const hiBits = Math.floor(bitLen / 0x100000000);
  const loBits = bitLen >>> 0;
  view.setUint32(paddedLen - 8, hiBits, false);
  view.setUint32(paddedLen - 4, loBits, false);

  let h0 = 0x67452301;
  let h1 = 0xefcdab89;
  let h2 = 0x98badcfe;
  let h3 = 0x10325476;
  let h4 = 0xc3d2e1f0;

  const w = new Uint32Array(80);

  for (let offset = 0; offset < paddedLen; offset += 64) {
    for (let i = 0; i < 16; i++) {
      w[i] = view.getUint32(offset + i * 4, false);
    }
    for (let i = 16; i < 80; i++) {
      const val = (w[i - 3]! ^ w[i - 8]! ^ w[i - 14]! ^ w[i - 16]!) >>> 0;
      w[i] = rotl32(val, 1);
    }

    let a = h0;
    let b = h1;
    let c = h2;
    let d = h3;
    let e = h4;

    for (let i = 0; i < 80; i++) {
      let f: number;
      let k: number;
      if (i < 20) {
        f = ((b & c) | (~b & d)) >>> 0;
        k = 0x5a827999;
      } else if (i < 40) {
        f = (b ^ c ^ d) >>> 0;
        k = 0x6ed9eba1;
      } else if (i < 60) {
        f = ((b & c) | (b & d) | (c & d)) >>> 0;
        k = 0x8f1bbcdc;
      } else {
        f = (b ^ c ^ d) >>> 0;
        k = 0xca62c1d6;
      }

      const temp = (rotl32(a, 5) + f + e + k + w[i]!) >>> 0;
      e = d;
      d = c;
      c = rotl32(b, 30);
      b = a;
      a = temp;
    }

    h0 = (h0 + a) >>> 0;
    h1 = (h1 + b) >>> 0;
    h2 = (h2 + c) >>> 0;
    h3 = (h3 + d) >>> 0;
    h4 = (h4 + e) >>> 0;
  }

  return [h0, h1, h2, h3, h4].map((h) => h.toString(16).padStart(8, "0")).join("");
}

const FNV_OFFSET_64 = 0xcbf29ce484222325n;
const FNV_PRIME_64 = 0x100000001b3n;
const MASK_64 = 0xffffffffffffffffn;

/**
 * Deterministic 64-bit FNV-1a hash matching `skotos_domain::big_segment_key_hash`.
 */
export function bigSegmentKeyHash(contextKind: string, contextKey: string): bigint {
  const bytes = textEncoder.encode(`${contextKind}:${contextKey}`);
  let hash = FNV_OFFSET_64;
  for (let i = 0; i < bytes.length; i++) {
    hash ^= BigInt(bytes[i]!);
    hash = (hash * FNV_PRIME_64) & MASK_64;
  }
  return hash;
}
