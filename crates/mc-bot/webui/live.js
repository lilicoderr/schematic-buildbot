// Live progress: poll the status and show it; the 3D preview can follow the build.
const $ = (id) => document.getElementById(id)
const ACTIVE = ['starting', 'building', 'stopping']

export function watch({ api, t, duration, fail, update, followable }) {
  let next = 0, shown = -1, wasDown = false
  async function poll() {
    let s
    try {
      s = await api(`/api/status?since=${next}`)
      if (wasDown) { fail(''); wasDown = false }
    } catch (e) {
      wasDown = true
      fail(e.message)
      return setTimeout(poll, 3000)
    }
    next = s.next
    update(s)
    $('phase').textContent = t.phases[s.phase] || ''
    if (s.steps > 0) {
      $('bar').value = s.percent
      $('percent').textContent = `${s.percent}%`
      $('counts').textContent = t.counts(s)
    } else {
      // no value: the bar shows that something is going on
      if (ACTIVE.includes(s.phase)) $('bar').removeAttribute('value')
      else $('bar').value = 0
      $('percent').textContent = '-'
      $('counts').textContent = ''
    }
    $('rate').textContent = s.phase === 'building' ? s.blocks_per_s : '-'
    $('eta').textContent = duration(s.eta_s)
    $('repairs').textContent = s.phase === 'idle' ? '-' : s.repairs
    if (s.log.length) {
      const log = $('log')
      const atEnd = log.scrollHeight - log.scrollTop - log.clientHeight < 30
      log.textContent = (log.textContent + s.log.join('\n') + '\n').split('\n').slice(-1001).join('\n')
      if (atEnd) log.scrollTop = log.scrollHeight
    }
    // only when the build runs the very plan on screen (another server version may plan differently)
    if ($('follow').checked && s.steps && s.steps === followable() && s.step !== shown && window.preview) {
      shown = s.step
      $('layer').value = $('layer').max
      window.preview.set({ step: s.step })
    }
    if (!ACTIVE.includes(s.phase)) shown = -1
    setTimeout(poll, ACTIVE.includes(s.phase) ? 600 : 2000)
  }
  poll()
}
