const uidPattern = /^[1-9]\d{0,19}$/

export function projectLiveTransition(live, previous) {
  const observedBefore = typeof previous?.live === 'boolean'
  return { notify: observedBefore && previous.live !== live }
}

function positiveInteger(value) {
  return Number.isSafeInteger(value) && value > 0
}

function shortText(value, fallback, limit) {
  if (typeof value !== 'string') return fallback
  const normalized = value.replace(/[\u0000-\u001f\u007f]/g, '').trim()
  return normalized ? [...normalized].slice(0, limit).join('') : fallback
}

function parseResponse(response, uid) {
  if (!Number.isInteger(response?.status) || response.status < 200 || response.status >= 300) {
    throw new Error(`B 站接口请求失败（HTTP ${Number.isInteger(response?.status) ? response.status : '未知'}）。`)
  }
  let payload
  try { payload = JSON.parse(response.text) } catch { throw new Error('B 站接口返回了无效数据。') }
  const data = payload?.data?.[uid]
  if (payload?.code !== 0 || !data || typeof data !== 'object') {
    throw new Error('B 站接口未返回有效主播信息。')
  }
  const { room_id: roomId, live_status: status } = data
  if (!positiveInteger(roomId) || ![0, 1, 2].includes(status)) {
    throw new Error('B 站接口返回的直播间或开播状态无效。')
  }
  const name = shortText(data.uname, '', 64)
  if (!name) throw new Error('B 站接口未返回 UP 主昵称。')
  return {
    live: status === 1,
    roomId,
    name,
    title: shortText(data.title, status === 1 ? '正在直播' : '当前未开播', 120),
  }
}

export async function collectBilibili(settings, context, previousState) {
  const uid = settings?.BILIBILI_UID?.trim()
  if (typeof uid !== 'string' || !uidPattern.test(uid)) throw new Error('请填写有效的 B 站用户 UID。')
  const response = await context.http.request('bilibiliApi', {
    method: 'GET',
    path: `/room/v1/Room/get_status_info_by_uids?uids%5B%5D=${uid}`,
    headers: { Accept: 'application/json', 'User-Agent': 'Mado-Studio/1.0' },
  })
  const room = parseResponse(response, uid)
  const transition = projectLiveTransition(room.live, previousState)
  const url = `https://live.bilibili.com/${room.roomId}`
  const outputs = {
    liveStatus: {
      state: room.live ? 'live' : 'offline',
      detail: room.name,
      details: { title: room.title, url },
    },
  }
  const desktopNotifications = transition.notify
    ? [{
        title: room.live ? 'B 站主播开播' : 'B 站主播下播',
        body: room.live
          ? `${room.name}：${room.title} · ${url}`
          : `${room.name} 已下播 · ${url}`,
      }]
    : []
  return {
    outputs,
    state: { live: room.live },
    notifications: transition.notify ? ['liveStatus'] : [],
    desktopNotifications,
  }
}
