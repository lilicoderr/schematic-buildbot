// Spectator-camera screenshots of a build, rendered by prismarine-viewer in headless Chromium.
//   node capture.js <outDir> <prefix> <x> <y> <z> <yaw> <pitch> [count=1] [intervalSec=10]
// RCON settings come from the environment (never from the repo):
//   RCON_PORT (default 25576), RCON_PASSWORD, MC_PORT (default 25566)
const mineflayer = require('mineflayer')
const { mineflayer: mineflayerViewer } = require('prismarine-viewer')
const { chromium } = require('playwright-core')
const net = require('net')
const fs = require('fs')
const path = require('path')

const [outDir, prefix, x, y, z, yaw, pitch, countArg, intervalArg] = process.argv.slice(2)
const count = Number(countArg || 1)
const intervalMs = Number(intervalArg || 10) * 1000
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

function rcon(cmd) {
  return new Promise((resolve, reject) => {
    const s = net.connect(Number(process.env.RCON_PORT || 25576), '127.0.0.1')
    const pkt = (id, type, body) => {
      const b = Buffer.alloc(14 + Buffer.byteLength(body))
      b.writeInt32LE(10 + Buffer.byteLength(body), 0); b.writeInt32LE(id, 4); b.writeInt32LE(type, 8)
      b.write(body, 12); return b
    }
    let out = ''
    s.on('data', (d) => { out += d.toString('latin1').replace(/[^ -~]/g, '') })
    s.on('connect', () => { s.write(pkt(1, 3, process.env.RCON_PASSWORD || '')); setTimeout(() => s.write(pkt(2, 2, cmd)), 150) })
    setTimeout(() => { s.end(); if (process.env.VIEWER_DEBUG) console.log('[rcon]', cmd, '->', out); resolve() }, 700)
    s.on('error', reject)
  })
}

async function main() {
  fs.mkdirSync(outDir, { recursive: true })
  const bot = mineflayer.createBot({ host: '127.0.0.1', port: Number(process.env.MC_PORT || 25566), username: 'Viewer', version: '1.21.1' })
  await new Promise((res, rej) => { bot.once('spawn', res); bot.once('error', rej); bot.once('kicked', rej) })
  bot.physicsEnabled = false
  await rcon('gamemode spectator Viewer')
  await rcon(`tp Viewer ${x} ${y} ${z} ${yaw} ${pitch}`)
  mineflayerViewer(bot, { port: 3007, firstPerson: true, viewDistance: 8 })
  const browser = await chromium.launch()
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } })
  page.on('console', (m) => { if (process.env.VIEWER_DEBUG) console.log('[page]', m.text().slice(0, 200)) })
  page.on('pageerror', (e) => console.log('[pageerror]', String(e).slice(0, 300)))
  if (process.env.VIEWER_DEBUG) {
    let frames = 0
    page.on('websocket', (ws) => ws.on('framereceived', () => { frames++ }))
    page.on('worker', (w) => console.log('[worker]', w.url()))
    setInterval(() => console.log('[ws frames]', frames), 5000).unref()
  }
  await page.goto('http://localhost:3007')
  for (let i = 0; i < count; i++) {
    await sleep(i === 0 ? Number(process.env.FIRST_WAIT_MS || 8000) : intervalMs)
    const file = path.join(outDir, `${prefix}${count > 1 ? '_' + String(i).padStart(3, '0') : ''}.png`)
    if (process.env.VIEWER_DEBUG) console.log('cols', Object.keys(bot.world.async.columns || bot.world.columns || {}).length, 'ground', bot.blockAt(bot.entity.position.offset(0, -101, 0))?.name, 'pos', bot.entity.position)
    await page.screenshot({ path: file })
    console.log('saved', file)
  }
  await browser.close()
  bot.end()
  process.exit(0)
}
main().catch((e) => { console.error(e); process.exit(1) })
