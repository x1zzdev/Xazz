import assert from 'node:assert/strict'
import {
  access,
  copyFile,
  mkdir,
  readFile,
  rm,
  writeFile,
} from 'node:fs/promises'
import { dirname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build } from 'vite'

const prototypeRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const distRoot = resolve(prototypeRoot, 'dist')
assert(
  distRoot.startsWith(`${prototypeRoot}${sep}`),
  'dist target must remain inside visual-ide',
)

const result = await build({
  logLevel: 'warn',
  build: {
    write: false,
  },
})
const outputs = Array.isArray(result)
  ? result.flatMap((item) => item.output ?? [])
  : result.output ?? []

const names = outputs.map((item) => item.fileName)
assert(names.includes('index.html'), 'Vite build did not emit index.html')
assert(names.some((name) => name.endsWith('.js')), 'Vite build did not emit JavaScript')
assert(names.some((name) => name.endsWith('.css')), 'Vite build did not emit CSS')

await rm(distRoot, { recursive: true, force: true })
await mkdir(distRoot, { recursive: true })

for (const output of outputs) {
  const target = resolve(distRoot, output.fileName)
  assert(
    target.startsWith(`${distRoot}${sep}`),
    `build output escaped dist: ${output.fileName}`,
  )
  await mkdir(dirname(target), { recursive: true })
  const contents = output.type === 'chunk' ? output.code : output.source
  await writeFile(target, contents)
}

await copyFile(
  resolve(prototypeRoot, 'public', 'favicon.svg'),
  resolve(distRoot, 'favicon.svg'),
)
await access(resolve(distRoot, 'favicon.svg'))
const html = await readFile(resolve(distRoot, 'index.html'), 'utf8')
assert.match(html, /favicon\.svg/, 'built HTML must keep the local favicon')

console.log(`build-gate: ok; outputs=${names.length}; local-favicon=present`)
