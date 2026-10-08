// Rasterizes glyph.svg to glyph.png with @resvg/resvg-js 2.6.2, independently of the Go renderer:
//   bun rasterize.mjs glyph.svg glyph.png
import fs from 'fs'
import { Resvg } from '@resvg/resvg-js'
const svg = fs.readFileSync(process.argv[2], 'utf8')
fs.writeFileSync(process.argv[3], new Resvg(svg, { fitTo: { mode: 'width', value: 44 } }).render().asPng())
