import fs from 'fs'
import { Resvg } from '@resvg/resvg-js'
const src = fs.readFileSync(process.argv[2], 'utf8')
const out = process.argv[3]
const svgs = src.match(/<svg.*?<\/svg>/gs)
const names = [
  'synced',
  'transferring',
  'new',
  'paused',
  'lan-only',
  'offline',
  'not-recording',
  'locked',
  'attention',
]
const m = (24 * (44 - 36)) / 2 / 36 // margin in design units for a 44px canvas with a 36px art box
const vb = `${-m} ${-m} ${24 + 2 * m} ${24 + 2 * m}`
const fgBg = ['#1D1D1F', '#E9E9EC']
names.forEach((n, k) => {
  const svg = svgs[6 * k + 1] // macOS light 18px variant
  const inner = svg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, '')
  const full = `<svg xmlns="http://www.w3.org/2000/svg" width="44" height="44" viewBox="${vb}"><rect x="${-m}" y="${-m}" width="${24 + 2 * m}" height="${24 + 2 * m}" fill="${fgBg[1]}"/>${inner}</svg>`
  fs.writeFileSync(`${out}/${n}.svg`, full)
  fs.writeFileSync(
    `${out}/${n}.png`,
    new Resvg(full, { fitTo: { mode: 'width', value: 44 } }).render().asPng()
  )
})
