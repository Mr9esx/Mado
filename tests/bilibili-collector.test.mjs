import assert from 'node:assert/strict'
import test from 'node:test'
import { collectBilibili, projectLiveTransition } from '../shared/collectors/bilibili/client.mjs'

const response = value => ({ status: 200, text: JSON.stringify(value) })
const room = (liveStatus, values = {}) => ({
  code: 0,
  data: {
    '998877': { room_id: 123456, uname: '样例UP主', title: '直播标题', live_status: liveStatus, ...values },
  },
})
function api(reply) {
  const calls = []
  return {
    calls,
    context: {
      http: {
        async request(alias, options) {
          calls.push({ alias, ...options })
          return typeof reply === 'function' ? reply(options) : reply
        },
      },
    },
  }
}

const settings = { BILIBILI_UID: '998877' }

test('first sample reports existing live status without notifying', async () => {
  const { context, calls } = api(response(room(1)))
  const result = await collectBilibili(settings, context, null)
  assert.equal(calls[0].path, '/room/v1/Room/get_status_info_by_uids?uids%5B%5D=998877')
  assert.equal(result.outputs.liveStatus.state, 'live')
  assert.equal(result.outputs.liveStatus.detail, '样例UP主')
  assert.equal(result.outputs.liveStatus.details.title, '直播标题')
  assert.deepEqual(result.desktopNotifications, [])
  assert.deepEqual(result.notifications, [])
})

test('offline to live notifies exactly once and repeated live samples stay quiet', async () => {
  const { context } = api(response(room(1)))
  const first = await collectBilibili(settings, context, { live: false })
  const repeated = await collectBilibili(settings, context, first.state)
  assert.equal(first.desktopNotifications.length, 1)
  assert.match(first.desktopNotifications[0].body, /直播标题/)
  assert.equal(repeated.desktopNotifications.length, 0)
})

test('live to offline notifies once and later live transition can notify again', async () => {
  const offline = api(response(room(0)))
  const stopped = await collectBilibili(settings, offline.context, { live: true })
  assert.equal(stopped.outputs.liveStatus.state, 'offline')
  assert.equal(stopped.outputs.liveStatus.detail, '样例UP主')
  assert.equal(stopped.desktopNotifications.length, 1)
  assert.match(stopped.desktopNotifications[0].title, /下播/)

  const online = api(response(room(1)))
  const resumed = await collectBilibili(settings, online.context, stopped.state)
  assert.equal(resumed.desktopNotifications.length, 1)
  assert.match(resumed.desktopNotifications[0].title, /开播/)
})

test('failed sampling rejects without replacing prior state', async () => {
  const { context } = api(response({ code: -1, message: 'bad uid' }))
  const previous = { live: true }
  await assert.rejects(() => collectBilibili(settings, context, previous))
  assert.equal(previous.live, true)
})

test('validates UID and API response before producing a state', async () => {
  const { context } = api(response(room(1)))
  await assert.rejects(() => collectBilibili({ BILIBILI_UID: '998877/evil' }, context, null))
  await assert.rejects(() => collectBilibili(settings, api(response(room(3))).context, null))
  await assert.rejects(() => collectBilibili(settings, api(response(room(1, { uname: ' ' }))).context, null), /昵称/)
})

test('recorded playback keeps the UP name and is shown as offline', async () => {
  const result = await collectBilibili(settings, api(response(room(2))).context, null)
  assert.equal(result.outputs.liveStatus.state, 'offline')
  assert.equal(result.outputs.liveStatus.detail, '样例UP主')
})

test('projects both state edges while the first and unchanged samples are silent', () => {
  assert.equal(projectLiveTransition(true, null).notify, false)
  assert.equal(projectLiveTransition(true, { live: false }).notify, true)
  assert.equal(projectLiveTransition(false, { live: true }).notify, true)
  assert.equal(projectLiveTransition(true, { live: true }).notify, false)
  assert.equal(projectLiveTransition(false, { live: false }).notify, false)
})
