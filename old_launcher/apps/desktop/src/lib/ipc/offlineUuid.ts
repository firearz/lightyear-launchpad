/**
 * Exact implementation of Java's `UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(UTF_8))`
 * RFC 4122 version 3 (MD5) with variant 2 (IETF).
 */
export function computeOfflinePlayerUuid(username: string): {
  dashed: string;
  undashed: string;
  validStrictUsername: boolean;
} {
  const input = new TextEncoder().encode(`OfflinePlayer:${username}`);
  const digest = md5Bytes(input);

  // Set version to 3 (name-based MD5)
  digest[6] = (digest[6] & 0x0f) | 0x30;
  // Set variant to IETF RFC 4122 (10xxxxxx)
  digest[8] = (digest[8] & 0x3f) | 0x80;

  const hex = Array.from(digest, (b) => b.toString(16).padStart(2, '0')).join('');
  const dashed = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
  const validStrictUsername = /^[A-Za-z0-9_]{3,16}$/.test(username);

  return { dashed, undashed: hex, validStrictUsername };
}

function md5Bytes(message: Uint8Array): Uint8Array {
  const s = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
    5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
    6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
  ];
  const K = new Uint32Array(64);
  for (let i = 0; i < 64; i++) {
    K[i] = Math.floor(Math.abs(Math.sin(i + 1)) * 0x100000000) >>> 0;
  }

  const origBitLen = message.length * 8;
  const padLen = ((56 - ((message.length + 1) % 64)) + 64) % 64;
  const totalLen = message.length + 1 + padLen + 8;
  const buf = new Uint8Array(totalLen);
  buf.set(message, 0);
  buf[message.length] = 0x80;

  const view = new DataView(buf.buffer);
  view.setUint32(totalLen - 8, origBitLen >>> 0, true);
  view.setUint32(totalLen - 4, Math.floor(origBitLen / 0x100000000) >>> 0, true);

  let a0 = 0x67452301;
  let b0 = 0xefcdab89;
  let c0 = 0x98badcfe;
  let d0 = 0x10325476;

  const M = new Uint32Array(16);
  for (let offset = 0; offset < totalLen; offset += 64) {
    for (let j = 0; j < 16; j++) {
      M[j] = view.getUint32(offset + j * 4, true);
    }
    let A = a0;
    let B = b0;
    let C = c0;
    let D = d0;

    for (let i = 0; i < 64; i++) {
      let F = 0;
      let g = 0;
      if (i < 16) {
        F = (B & C) | (~B & D);
        g = i;
      } else if (i < 32) {
        F = (D & B) | (~D & C);
        g = (5 * i + 1) % 16;
      } else if (i < 48) {
        F = B ^ C ^ D;
        g = (3 * i + 5) % 16;
      } else {
        F = C ^ (B | ~D);
        g = (7 * i) % 16;
      }
      F = (F + A + K[i] + M[g]) >>> 0;
      A = D;
      D = C;
      C = B;
      const rotated = ((F << s[i]) | (F >>> (32 - s[i]))) >>> 0;
      B = (B + rotated) >>> 0;
    }

    a0 = (a0 + A) >>> 0;
    b0 = (b0 + B) >>> 0;
    c0 = (c0 + C) >>> 0;
    d0 = (d0 + D) >>> 0;
  }

  const out = new Uint8Array(16);
  const outView = new DataView(out.buffer);
  outView.setUint32(0, a0, true);
  outView.setUint32(4, b0, true);
  outView.setUint32(8, c0, true);
  outView.setUint32(12, d0, true);
  return out;
}
