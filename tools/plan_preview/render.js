// Screenshots and GIF frames of the plan preview, taken in headless Chromium.
//   node render.js <preview.html built by build.py> <imageDir> <frameDir>
const path = require('path')
const fs = require('fs')
const { pathToFileURL } = require('url')
const { chromium } = require('playwright-core')

async function main() {
  const [html, imageDir, frameDir] = process.argv.slice(2)
  if (!frameDir) throw new Error('usage: node render.js preview.html imageDir frameDir')
  for (const d of [imageDir, frameDir]) fs.mkdirSync(d, { recursive: true })
  const browser = await chromium.launch({ args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] })
  const page = await browser.newPage({ viewport: { width: 1200, height: 720 } })
  const errors = []
  page.on('pageerror', (e) => errors.push(String(e)))
  await page.goto(pathToFileURL(path.resolve(html)).href)
  await page.waitForFunction(() => window.preview, null, { timeout: 60000 })
  const { steps, run } = await page.evaluate(() => {
    const { steps, run } = window.preview.info()
    const counts = run.counts.map((c) => c.scaffold)
    const at = counts.indexOf(Math.max(...counts)) + 1
    const temp = run.blocks.filter((b) => b.kind !== 'block' && b.from <= at && at < b.to)
    const level = (list) => Math.max(...list.map((b) => b.pos[1])) - run.ground
    return { steps, run: { counts, top: level(run.blocks), cut: level(temp) } }
  })
  // The step with the most scaffold standing shows the temporary blocks best.
  const busy = run.counts.indexOf(Math.max(...run.counts)) + 1
  const shot = async (dir, name, view) => {
    await page.evaluate((v) => window.preview.set(v), view)
    await page.screenshot({ path: path.join(dir, name) })
  }
  const cam = { azimuth: 35, elevation: 40, zoom: 1.05 }
  await shot(imageDir, 'plan-preview-scaffold.png', { ...cam, step: busy, layer: run.cut })
  await shot(imageDir, 'plan-preview-layer.png', { ...cam, step: steps, layer: Math.floor(run.top / 3), elevation: 55 })
  const FRAMES = 48
  await page.evaluate((v) => window.preview.set(v), { ...cam, layer: run.top })
  for (let i = 0; i < FRAMES; i++) {
    await shot(frameDir, `frame_${String(i).padStart(3, '0')}.png`, { step: Math.round((steps * i) / (FRAMES - 1)) })
  }
  await browser.close()
  if (errors.length) throw new Error(errors.join('\n'))
  console.log(`steps ${steps}, most scaffold at step ${busy}`)
}
main().catch((e) => { console.error(e); process.exit(1) })
