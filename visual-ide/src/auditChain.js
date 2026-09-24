/**
 * Browser-side replay of xazz-server audit_log.rs::verify_chain.
 *
 * The server's `GET /security/audit/chain` verdict stays authoritative; it only says
 * intact true/false. This replay exists to name *which* record breaks the chain, so
 * a tampered log can be explained, not just flagged. Keep the field order and the
 * NUL separators identical to compute_record_hash or every record will mismatch.
 */
const OPTIONAL_FIELDS = ['outcome', 'prompt_hash', 'response_hash', 'model_fingerprint']

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')
}

export function recordHash(record) {
  const parts = [String(record.index), record.timestamp, record.hash, String(record.code_length)]
  for (const field of OPTIONAL_FIELDS) {
    if (record[field] != null) parts.push(record[field])
  }
  return sha256Hex(`${parts.map((part) => `${part}\0`).join('')}${record.prev_hash}`)
}

/**
 * @returns {Promise<null | {position: number, index: number, reason: 'link' | 'hash'}>}
 *   null when every record links to its predecessor and recomputes to its record_hash.
 */
export async function findFirstBreak(records) {
  let prev = 'GENESIS'
  for (const [position, record] of records.entries()) {
    if (record.prev_hash !== prev) return { position, index: record.index, reason: 'link' }
    if ((await recordHash(record)) !== record.record_hash) {
      return { position, index: record.index, reason: 'hash' }
    }
    prev = record.record_hash
  }
  return null
}
