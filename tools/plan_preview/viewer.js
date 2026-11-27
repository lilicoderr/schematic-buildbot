import * as THREE from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'

const $ = (id) => document.getElementById(id)
const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true })
$('view').appendChild(renderer.domElement)
const scene = new THREE.Scene()
scene.background = new THREE.Color(0x181c26)
const camera = new THREE.PerspectiveCamera(35, 1, 0.1, 2000)
const controls = new OrbitControls(camera, renderer.domElement)
scene.add(new THREE.HemisphereLight(0xffffff, 0x404050, 2.2))
const sun = new THREE.DirectionalLight(0xffffff, 1.4)
sun.position.set(0.6, 1, 0.3)
scene.add(sun)

const cube = new THREE.BoxGeometry(0.96, 0.96, 0.96)
const small = new THREE.BoxGeometry(0.7, 0.96, 0.7)
const edges = new THREE.EdgesGeometry(new THREE.BoxGeometry(1.08, 1.08, 1.08))
const botMesh = new THREE.Mesh(new THREE.BoxGeometry(0.6, 1.8, 0.6), new THREE.MeshBasicMaterial({ color: 0x35e0ff }))
const mark = new THREE.LineSegments(edges, new THREE.LineBasicMaterial({ color: 0xff3df0, depthTest: false }))
const path = new THREE.Line(new THREE.BufferGeometry(), new THREE.LineBasicMaterial({ color: 0x35e0ff, depthTest: false }))
mark.renderOrder = path.renderOrder = 2
path.frustumCulled = false
scene.add(botMesh, mark, path)

let plan, run, meshes = [], grid, timer = null
const colours = (window.EMBED && window.EMBED.colours) || {}
function colour(name) {
  if (colours[name]) return colours[name]
  let h = 0
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) >>> 0
  return [70 + (h % 160), 70 + ((h >> 8) % 160), 70 + ((h >> 16) % 160)]
}
const centre = (p) => new THREE.Vector3(p[0] + 0.5, p[1] + 0.5, p[2] + 0.5)

function instances(list, geo, material, tint) {
  const m = new THREE.InstancedMesh(geo, material, list.length)
  const mat = new THREE.Matrix4(), col = new THREE.Color()
  list.forEach((b, i) => {
    m.setMatrixAt(i, mat.setPosition(centre(b.pos)))
    if (tint) m.setColorAt(i, col.setRGB(...colour(b.state.split('[')[0]).map((v) => v / 255), THREE.SRGBColorSpace))
  })
  m.frustumCulled = false
  if (!tint) m.renderOrder = 1
  scene.add(m)
  meshes.push(m)
}
const solid = new THREE.MeshLambertMaterial()
const supportMat = new THREE.MeshBasicMaterial({ color: 0xff7a1a, transparent: true, opacity: 0.75, depthTest: false })
const pillarMat = new THREE.MeshBasicMaterial({ color: 0xffe14d, transparent: true, opacity: 0.75, depthTest: false })
const outline = new THREE.LineBasicMaterial({ color: 0xffffff, depthTest: false })

function draw() {
  if (!run) return
  const step = +$('step').value, layer = +$('layer').value, only = $('only').checked
  meshes.forEach((m) => { scene.remove(m); if (m.isInstancedMesh) m.dispose() })
  meshes = []
  const shown = (y) => (only ? y - run.ground === layer : y - run.ground <= layer)
  const standing = run.blocks.filter((b) => b.from <= step && step < b.to && shown(b.pos[1]))
  instances(standing.filter((b) => b.kind === 'block'), cube, solid, true)
  instances(standing.filter((b) => b.kind === 'support'), small, supportMat)
  instances(standing.filter((b) => b.kind === 'pillar'), small, pillarMat)
  // Temporary blocks get a white outline that is drawn through walls, so they cannot be
  // missed or confused with orange / yellow schematic blocks.
  for (const b of standing.filter((b) => b.kind !== 'block')) {
    const line = new THREE.LineSegments(edges, outline)
    line.position.copy(centre(b.pos))
    line.renderOrder = 1
    scene.add(line)
    meshes.push(line)
  }
  const s = plan.steps[step - 1]
  const feet = step ? run.bot[step - 1] : plan.header.start || run.bot[0]
  botMesh.position.set(feet[0] + 0.5, feet[1] + 0.9, feet[2] + 0.5)
  mark.visible = !!(s && s.pos)
  if (mark.visible) mark.position.copy(centre(s.pos))
  const trail = run.bot.slice(Math.max(0, step - 60), step).map((p) => new THREE.Vector3(p[0] + 0.5, p[1] + 0.15, p[2] + 0.5))
  path.geometry.dispose()
  path.geometry = new THREE.BufferGeometry().setFromPoints(trail)
  const c = step ? run.counts[step - 1] : { placed: 0, scaffold: 0, moves: 0, pillars: 0, supports: 0, breaks: 0 }
  table('live', {
    'blocks placed': `${c.placed} / ${run.total.placed}`, 'scaffold standing': c.scaffold, moves: c.moves,
    'pillar blocks used': c.pillars, 'supports used': c.supports, 'scaffold breaks': c.breaks,
    'bot feet': feet.join(' '),
  })
  $('cur').textContent = s ? s.line : '(before the first step)'
  $('stepv').textContent = `${step} / ${plan.steps.length}`
  $('layerv').textContent = `${layer} / ${$('layer').max}`
  renderer.render(scene, camera)
}
function table(id, obj) {
  $(id).replaceChildren(...Object.entries(obj).map(([k, v]) => {
    const tr = document.createElement('tr')
    for (const text of [k, v]) tr.appendChild(document.createElement('td')).textContent = text
    return tr
  }))
}

function resize() {
  const { clientWidth: w, clientHeight: h } = $('view')
  renderer.setSize(w, h)
  camera.aspect = w / h
  camera.updateProjectionMatrix()
  draw()
}
function orbit(azimuth, elevation = 38, zoom = 1) {
  const box = new THREE.Box3()
  run.blocks.filter((b) => b.kind === 'block').forEach((b) => box.expandByPoint(centre(b.pos)))
  const sphere = box.getBoundingSphere(new THREE.Sphere())
  const d = (sphere.radius * 2.9) / zoom, a = THREE.MathUtils.degToRad(azimuth), e = THREE.MathUtils.degToRad(elevation)
  controls.target.copy(sphere.center)
  camera.position.set(d * Math.cos(e) * Math.sin(a), d * Math.sin(e), d * Math.cos(e) * Math.cos(a)).add(sphere.center)
  controls.update()
  draw()
}
function load(text) {
  plan = parsePlan(text)
  run = replay(plan)
  const top = Math.max(...run.blocks.map((b) => b.pos[1])) - run.ground
  Object.assign($('step'), { max: plan.steps.length, value: plan.steps.length })
  Object.assign($('layer'), { max: top, value: top })
  table('stats', { ...plan.header.stats, unreachable: plan.header.unreachable })
  if (grid) scene.remove(grid)
  grid = new THREE.GridHelper(96, 96, 0x465070, 0x2b3246)
  const o = plan.header.origin
  grid.position.set(o[0] + 16, run.ground, o[2] + 16)
  scene.add(grid)
  orbit(35)
}
function stop() { clearInterval(timer); timer = null; $('play').textContent = 'Play' }
$('play').onclick = () => {
  if (timer) return stop()
  if (+$('step').value >= plan.steps.length) $('step').value = 0
  $('play').textContent = 'Pause'
  timer = setInterval(() => {
    $('step').value = +$('step').value + Math.ceil(plan.steps.length / 600)
    draw()
    if (+$('step').value >= plan.steps.length) stop()
  }, 50)
}
$('step').oninput = $('layer').oninput = $('only').onchange = draw
$('file').onchange = async (e) => {
  stop()
  try { load(await e.target.files[0].text()) } catch (err) { $('cur').textContent = String(err) }
}
controls.addEventListener('change', () => renderer.render(scene, camera))
window.addEventListener('resize', resize)
resize()
if (window.EMBED && window.EMBED.plan) load(window.EMBED.plan)

// Used by render.js to take screenshots.
window.preview = {
  set({ step, layer, azimuth, elevation, zoom }) {
    if (step !== undefined) $('step').value = step
    if (layer !== undefined) $('layer').value = layer
    if (azimuth !== undefined) orbit(azimuth, elevation, zoom)
    else draw()
  },
  info: () => ({ steps: plan.steps.length, run }),
}
