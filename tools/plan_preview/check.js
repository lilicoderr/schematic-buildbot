// node check.js plan.bbplan : replays the plan and prints the final counts.
const { parsePlan, replay } = require('./plan.js')
const r = replay(parsePlan(require('fs').readFileSync(process.argv[2], 'utf8')))
console.log(r.total, 'highest layer', Math.max(...r.blocks.map((b) => b.pos[1])) - r.ground)
