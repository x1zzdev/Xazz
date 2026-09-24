// Cross-implementation check: the browser replay (src/auditChain.js) must reproduce
// record_hash values written by the real Rust server (audit_log.rs). The fixture is a
// verbatim GET /security/audit/log from a locally built xazz-server — it holds both
// plain execution records and an inference record with prompt/response/model hashes.
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { findFirstBreak, recordHash } from '../src/auditChain.js'

const fixture = JSON.parse(
  await readFile(new URL('./fixtures/audit-log.real.json', import.meta.url), 'utf8'),
)
const records = fixture.records
assert.ok(records.length >= 2, 'fixture must hold a real chain')
assert.ok(records.some((record) => record.prompt_hash), 'fixture must cover an inference record')

for (const record of records) {
  assert.equal(await recordHash(record), record.record_hash, `record ${record.index} must recompute`)
}
assert.equal(await findFirstBreak(records), null, 'an untouched server chain is intact')

// Tamper 1: rewrite a field in place — the link survives, the recomputed hash does not.
const edited = structuredClone(records)
edited[1].outcome = edited[1].outcome === 'success' ? 'failed' : 'success'
assert.deepEqual(await findFirstBreak(edited), { position: 1, index: records[1].index, reason: 'hash' })

// Tamper 2: delete a record — the next record's prev_hash no longer links.
const deleted = records.filter((_, position) => position !== 1)
assert.deepEqual(await findFirstBreak(deleted), { position: 1, index: records[2].index, reason: 'link' })

// Tamper 3: drop an optional field — absent fields are skipped, not hashed as empty.
const stripped = structuredClone(records)
const inference = stripped.findIndex((record) => record.model_fingerprint)
delete stripped[inference].model_fingerprint
assert.equal((await findFirstBreak(stripped))?.reason, 'hash')

console.log(`auditChain: ok; real records=${records.length}; tamper cases=3`)
