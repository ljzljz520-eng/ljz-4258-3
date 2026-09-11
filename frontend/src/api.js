// 平台 API 客户端。角色通过请求头传递:operator 标记事件,engineer 冻结配置。
let auth = { role: 'operator', user: 'op-1' };

export function setAuth(role, user) {
  auth = { role, user };
}

async function req(path, opts = {}) {
  const res = await fetch(path, {
    ...opts,
    headers: {
      'content-type': 'application/json',
      'x-role': auth.role,
      'x-user': auth.user,
    },
  });
  if (!res.ok) {
    let msg = res.statusText;
    try {
      msg = (await res.json()).error || msg;
    } catch { /* 忽略解析失败 */ }
    throw new Error(msg);
  }
  return res.json();
}

export const api = {
  batches: () => req('/api/batches'),
  createBatch: (product_name) =>
    req('/api/batches', { method: 'POST', body: JSON.stringify({ product_name }) }),
  closeBatch: (id) => req(`/api/batches/${id}/close`, { method: 'POST' }),
  timeline: (id) => req(`/api/batches/${id}/timeline`),
  evidence: (id, channel = 'all') => req(`/api/batches/${id}/evidence?channel=${channel}`),
  analyze: (id) => req(`/api/batches/${id}/analyze`, { method: 'POST' }),
  passage: (id, entry) =>
    req(`/api/batches/${id}/passage?entry=${encodeURIComponent(entry)}`),
  recirculation: (id) => req(`/api/batches/${id}/recirculation`),
  addEvent: (id, kind, note) =>
    req(`/api/batches/${id}/events`, { method: 'POST', body: JSON.stringify({ kind, note }) }),
  freezeTube: (volume_l, note) =>
    req('/api/configs/holding-tube', { method: 'POST', body: JSON.stringify({ volume_l, note }) }),
  freezeSpec: (body) => req('/api/configs/spec', { method: 'POST', body: JSON.stringify(body) }),
  freezeInstrument: (body) =>
    req('/api/configs/instruments', { method: 'POST', body: JSON.stringify(body) }),
  tubeConfigs: () => req('/api/configs/holding-tube'),
  instruments: () => req('/api/configs/instruments'),
  activeSpec: () => req('/api/configs/spec/active'),
  notices: () => req('/api/notices'),
  addNotice: (body) => req('/api/notices', { method: 'POST', body: JSON.stringify(body) }),
};
