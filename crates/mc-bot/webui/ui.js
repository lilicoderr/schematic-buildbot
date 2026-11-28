// Loading a schematic and starting or stopping a build. All text from files or the server is
// put on the page with textContent only.
import { watch } from './live.js'

const $ = (id) => document.getElementById(id)
let t = window.TEXTS.en
let analysis = null, source = null, phase = 'idle', loading = false, token = ''

// The key arrives in the link's fragment; keep it for reloads of this tab and hide it.
const fromLink = new URLSearchParams(location.hash.slice(1)).get('token')
try {
  if (fromLink) sessionStorage.setItem('token', fromLink)
  token = fromLink || sessionStorage.getItem('token') || ''
} catch { token = fromLink || '' }
if (location.hash) history.replaceState(null, '', location.pathname)

export async function api(path, { method = 'GET', body, type } = {}) {
  const headers = { 'X-BuildBot-Token': token }
  if (type) headers['Content-Type'] = type
  let res, data
  try {
    res = await fetch(path, { method, headers, body })
    data = await res.json()
  } catch { throw new Error(t.gone) }
  if (res.status === 401) throw new Error(t.badToken)
  if (!res.ok) throw new Error(data.error || res.statusText)
  return data
}
const postJson = (path, value) => api(path, { method: 'POST', type: 'application/json', body: JSON.stringify(value) })

function show(id, message) {
  $(id).hidden = !message
  $(id).textContent = message || ''
}
function list(id, items) {
  $(id).hidden = !items.length
  $(id).replaceChildren(...items.map((text) => Object.assign(document.createElement('li'), { textContent: text })))
}
function rows(id, data, classes = []) {
  $(id).replaceChildren(...data.map((cells, i) => {
    const tr = document.createElement('tr')
    tr.className = classes[i] || ''
    for (const text of cells) tr.appendChild(document.createElement('td')).textContent = text
    return tr
  }))
}
const duration = (s) => (s == null ? '-' : s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}m ${s % 60}s` : `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`)

// Flat block colours (as in tools/preview.py); unknown names get a stable colour in the viewer.
const COLOURS = {
  stone: [125, 125, 125], cobblestone: [110, 110, 110], glass: [170, 215, 230], dirt: [134, 96, 67], grass_block: [95, 159, 53],
  sand: [219, 207, 163], oak_planks: [162, 130, 78], spruce_planks: [114, 84, 48], birch_planks: [192, 175, 121], oak_log: [109, 85, 50],
  bricks: [150, 97, 83], stone_bricks: [122, 121, 122], white_concrete: [207, 213, 214], black_concrete: [8, 10, 15],
  gray_concrete: [55, 58, 62], light_gray_concrete: [125, 125, 115], red_concrete: [142, 33, 33], blue_concrete: [45, 47, 143],
  yellow_concrete: [240, 175, 21], green_concrete: [73, 91, 36], orange_concrete: [224, 97, 1], pink_concrete: [213, 101, 142],
  lime_concrete: [94, 168, 24], gold_block: [246, 208, 61], iron_block: [220, 220, 220], diamond_block: [98, 219, 214],
  glowstone: [171, 131, 84], obsidian: [20, 18, 30], netherrack: [97, 38, 38],
}
function colourOf(name) {
  const base = name.replace(/_(stairs|slab|wall|fence)$/, '')
  return [name, base, `${base}_planks`, `${base}s`, `${base}_block`].map((n) => COLOURS[n]).find(Boolean)
}

function describe(a) {
  rows('facts', [[t.fFile, a.file], [t.fSize, a.size.join(' x ')], [t.fBlocks, a.blocks], [t.fPlace, a.placements],
    [t.fTemp, a.temporary], [t.fTime, duration(a.est_seconds)], [t.fVersion, a.version]])
  rows('types', a.types.map(([n, c, ok]) => [ok ? n : `${n} (${t.missingItem})`, c]), a.types.map((x) => (x[2] ? '' : 'missing')))
  list('problems', a.problems)
  list('warnings', a.warnings)
  $('clean').hidden = a.problems.length > 0
  $('partialrow').hidden = !a.problems.length
  $('partial').checked = false
  $('summary').hidden = false
  if (a.plan && window.preview) {
    for (const [n] of a.types) if (colourOf(n)) window.EMBED.colours[n] = colourOf(n)
    window.preview.load(a.plan)
    $('empty').hidden = true
  }
}

function buttons() {
  const active = ['starting', 'building', 'stopping'].includes(phase)
  $('build').disabled = !analysis || active || loading || (analysis.problems.length > 0 && !$('partial').checked)
  $('stop').disabled = !active || phase === 'stopping'
  for (const id of ['pick', 'url', 'fetch', 'version', 'server', 'name', 'origin', 'fresh']) $(id).disabled = active || loading
}

// `source` is what was loaded last: a File or a link. It is loaded again when the version changes.
async function load(next) {
  if (!next || loading) return
  source = next
  analysis = null
  loading = true
  $('summary').hidden = true
  show('loaderror', '')
  show('loading', t.loading)
  buttons()
  try {
    const version = $('version').value
    if (typeof source === 'string') analysis = await postJson('/api/url', { url: source, version })
    else {
      if (source.size > 32 * 1024 * 1024) throw new Error(t.tooBig)
      const query = new URLSearchParams({ name: source.name, version })
      analysis = await api(`/api/upload?${query}`, { method: 'POST', type: 'application/octet-stream', body: source })
    }
    describe(analysis)
  } catch (e) {
    analysis = null
    show('loaderror', e.message)
  }
  loading = false
  show('loading', '')
  buttons()
}

$('pick').onchange = () => { load($('pick').files[0]); $('pick').value = '' }
$('fetch').onclick = () => load($('url').value.trim())
$('url').onkeydown = (e) => { if (e.key === 'Enter') $('fetch').click() }
$('version').onchange = () => load(source)
$('partial').onchange = buttons
// a file dropped anywhere is loaded instead of being opened by the browser
window.addEventListener('dragover', (e) => { e.preventDefault(); $('drop').classList.add('over') })
window.addEventListener('dragleave', () => $('drop').classList.remove('over'))
window.addEventListener('drop', (e) => {
  e.preventDefault()
  $('drop').classList.remove('over')
  if (!$('pick').disabled) load(e.dataTransfer.files[0])
})

$('build').onclick = async () => {
  const server = $('server').value.trim()
  if (!server) return show('loaderror', t.need)
  show('loaderror', '')
  try { localStorage.setItem('server', server); localStorage.setItem('name', $('name').value) } catch { /* optional */ }
  $('build').disabled = true
  try {
    await postJson('/api/build', {
      id: analysis.id, server, name: $('name').value.trim(), version: $('version').value, origin: $('origin').value.trim(),
      allow_partial: $('partial').checked, fresh: $('fresh').checked,
    })
    $('fresh').checked = false // pressing Build again after a stop must resume
    phase = 'starting'
  } catch (e) { show('loaderror', e.message) }
  buttons()
}
$('stop').onclick = async () => {
  $('stop').disabled = true
  try { await postJson('/api/stop', {}) } catch (e) { show('loaderror', e.message) }
}

async function start() {
  try { $('server').value = localStorage.getItem('server') || ''; $('name').value = localStorage.getItem('name') || 'BuildBot' } catch { /* optional */ }
  let info = null, problem = ''
  if (token) try { info = await api('/api/info') } catch (e) { problem = e.message }
  const lang = info ? info.lang : navigator.language.startsWith('ko') ? 'ko' : 'en'
  t = window.TEXTS[lang]
  document.documentElement.lang = lang
  for (const el of document.querySelectorAll('[data-t]')) el.textContent = t[el.dataset.t]
  if (!info) return show('fatal', problem || t.noToken)
  for (const v of info.versions) $('version').add(new Option(v, v))
  buttons()
  watch({
    api, t, duration, fail: (m) => show('fatal', m),
    update: (s) => { if (phase !== s.phase) { phase = s.phase; buttons() } },
    followable: () => analysis && analysis.steps,
  })
}
start()
