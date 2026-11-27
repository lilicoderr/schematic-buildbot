// Parses a .bbplan (docs/PLAN_FORMAT.md) and replays it. No DOM: also usable from node.
function parsePlan(text) {
  const header = { stats: {}, unreachable: 0, origin: [0, 0, 0], start: null }
  const steps = []
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim().split(/\s+/)
    if (t[0] === '#') {
      if (t[1] === 'stat') header.stats[t[2]] = t.slice(3).join(' ')
      else if (t[1] === 'unreachable') header.unreachable++
      else if (t[1] === 'origin' || t[1] === 'start') header[t[1]] = t.slice(2, 5).map(Number)
      else if (t[1] === 'bbplan' && t[2] !== '1') throw new Error('unsupported plan version ' + t[2])
    } else if (t[0]) {
      if (!['MOVE', 'PILLAR', 'PLACE', 'SUPPORT', 'AUTO', 'BREAK', 'CHECKPOINT'].includes(t[0])) throw new Error('unknown step ' + t[0])
      const pos = t[0] === 'CHECKPOINT' ? null : t.slice(1, 4).map(Number)
      const state = t[0] === 'PLACE' || t[0] === 'AUTO' ? t[4].replace('minecraft:', '') : null
      steps.push({ op: t[0], pos, state, line: line.trim() })
    }
  }
  if (!steps.length) throw new Error('no steps in plan')
  return { header, steps }
}

// Every block gets the step range [from, to) in which it stands (step numbers are 1-based).
// The bot position after each step is recorded; breaking the block under the bot makes it fall.
function replay(plan) {
  const blocks = [], live = new Map(), bot = [], counts = []
  let feet = plan.header.start || plan.steps.find((s) => s.op === 'MOVE').pos
  const ground = feet[1]
  const c = { placed: 0, scaffold: 0, moves: 0, pillars: 0, supports: 0, breaks: 0 }
  const add = (pos, kind, state, i) => {
    const b = { pos, kind, state, from: i, to: Infinity }
    blocks.push(b)
    live.set(pos.join(), b)
  }
  plan.steps.forEach((s, idx) => {
    const i = idx + 1
    if (s.op === 'MOVE') { feet = s.pos; c.moves++ }
    if (s.op === 'PILLAR') { add(s.pos, 'pillar', null, i); feet = [s.pos[0], s.pos[1] + 1, s.pos[2]]; c.pillars++; c.scaffold++ }
    if (s.op === 'SUPPORT') { add(s.pos, 'support', null, i); c.supports++; c.scaffold++ }
    if (s.op === 'PLACE' || s.op === 'AUTO') {
      const old = live.get(s.pos.join())
      if (old) old.to = i // second click on the same cell (double slab)
      else c.placed++
      add(s.pos, 'block', s.state, i)
    }
    if (s.op === 'BREAK') {
      const b = live.get(s.pos.join())
      if (!b || b.kind === 'block') throw new Error(`step ${i}: BREAK of a non-scaffold cell`)
      b.to = i; live.delete(s.pos.join()); c.breaks++; c.scaffold--
      if (feet[0] === s.pos[0] && feet[2] === s.pos[2] && feet[1] === s.pos[1] + 1) {
        let y = s.pos[1]
        while (y > ground && !live.has([feet[0], y - 1, feet[2]].join())) y--
        feet = [feet[0], y, feet[2]]
      }
    }
    bot.push(feet)
    counts.push({ ...c })
  })
  return { blocks, bot, counts, ground, total: { ...c } }
}
if (typeof module !== 'undefined') module.exports = { parsePlan, replay }
