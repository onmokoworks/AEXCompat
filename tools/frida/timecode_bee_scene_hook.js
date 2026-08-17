'use strict';
// Frida hook for AE Timecode.aex RENDER path (issue #1210 dynamic RE).
// Logs BEE.dll / U.dll calls made from Timecode.aex and dumps the BEE_Layer /
// BEE_Item / BEE_Project fields the plug-in and BEE.dll read.

function log(o) { send(o); }
function hex(p) { return p.toString(); }

let tc = null, bee = null, u = null;
let inRender = false;
const hookedVslots = new Set();

function modOff(p) {
  const m = Process.findModuleByAddress(p);
  if (!m) return hex(p);
  return m.name + '+0x' + p.sub(m.base).toString(16);
}
function inTimecode(ra) { return tc && ra.compare(tc.base) >= 0 && ra.compare(tc.base.add(tc.size)) < 0; }
function inBee(ra) { return bee && ra.compare(bee.base) >= 0 && ra.compare(bee.base.add(bee.size)) < 0; }

function safeBytes(p, n) {
  try { return Array.from(new Uint8Array(p.readByteArray(n))).map(b => ('0' + b.toString(16)).slice(-2)).join(''); }
  catch (e) { return '<unreadable ' + e + '>'; }
}
function rtti(obj) {
  try {
    const vt = obj.readPointer();
    const col = vt.sub(8).readPointer();
    const m = Process.findModuleByAddress(col);
    if (!m) return { vtable: modOff(vt), rtti: '?' };
    const tdRva = col.add(0xc).readU32();
    const td = m.base.add(tdRva);
    const name = td.add(0x10).readCString();
    return { vtable: modOff(vt), rtti: name, colOffset: col.add(4).readU32() };
  } catch (e) { return { vtable: '<err ' + e + '>' }; }
}
function vslot(obj, idx) {
  try { return modOff(obj.readPointer().add(idx * 8).readPointer()); } catch (e) { return '<err>'; }
}
function hookVslot(obj, idx, label) {
  let fn;
  try { fn = obj.readPointer().add(idx * 8).readPointer(); } catch (e) { return; }
  const key = fn.toString();
  if (hookedVslots.has(key)) return;
  hookedVslots.add(key);
  Interceptor.attach(fn, {
    onEnter(args) {
      this.ra = this.returnAddress;
      this.hit = inRender && (inTimecode(this.ra) || inBee(this.ra));
      this.a0 = args[0]; this.a1 = args[1]; this.a2 = args[2];
    },
    onLeave(rv) {
      if (!this.hit) return;
      log({ ev: 'vslot', slot: idx, label: label, fn: modOff(this.a0.readPointer().add(idx * 8).readPointer()), this: hex(this.a0), a1: hex(this.a1), a2: hex(this.a2), ret: hex(rv), from: modOff(this.ra) });
    }
  });
  log({ ev: 'vslot_hooked', slot: idx, label: label, fn: modOff(fn) });
}

function dumpItem(item, tag) {
  const o = { ev: 'item', tag: tag, item: hex(item) };
  try {
    o.rtti = rtti(item);
    o.tag_u16_at_8 = item.add(8).readU16().toString(16);
    o.type_s16_at_0x48 = item.add(0x48).readS16();
    o.flags_at_0x4c = '0x' + item.add(0x4c).readU32().toString(16);
    o.project_at_0x38 = hex(item.add(0x38).readPointer());
    o.bytes_0_0x60 = safeBytes(item, 0x60);
    const type = item.add(0x48).readS16();
    if (type === 4) {
      o.comp_0x260_0x2c0 = safeBytes(item.add(0x260), 0x60);
      o.comp_0x2f8_0x310 = safeBytes(item.add(0x2f8), 0x18);
      o.comp_0x410_0x430 = safeBytes(item.add(0x410), 0x20);
    } else if (type === 7) {
      const pinpp = item.add(0x2f8).readPointer();
      o.pin_pp = hex(pinpp);
      const pin = pinpp.readPointer();
      o.pin = hex(pin);
      o.pin_0x1e0_0x210 = safeBytes(pin.add(0x1e0), 0x30);
      o.pin_0x20_0x30 = safeBytes(pin.add(0x20), 0x10);
      const s = pin.add(0x28).readPointer();
      o.pin28 = hex(s);
      o.pin28_0_0x48 = safeBytes(s, 0x48);
      o.item_0x260_0x280 = safeBytes(item.add(0x260), 0x20);
    }
  } catch (e) { o.err = '' + e; }
  log(o);
}
function dumpProject(p, tag) {
  const o = { ev: 'project', tag: tag, project: hex(p) };
  try {
    o.rtti = rtti(p);
    o.tdf_0x84 = safeBytes(p.add(0x84), 0x14);
    o.bytes_0_0xa0 = safeBytes(p, 0xa0);
  } catch (e) { o.err = '' + e; }
  log(o);
}
function dumpLayer(layer, tag) {
  const o = { ev: 'layer', tag: tag, layer: hex(layer) };
  try {
    o.rtti = rtti(layer);
    o.slots = { s56: vslot(layer, 56), s65: vslot(layer, 65), s79: vslot(layer, 79), s183: vslot(layer, 183), s184: vslot(layer, 184) };
    o.item_0x260 = hex(layer.add(0x260).readPointer());
    o.bytes_0x240_0x2a0 = safeBytes(layer.add(0x240), 0x60);
    o.bytes_0_0x40 = safeBytes(layer, 0x40);
    // vtable size guess: count entries until pointer not in BEE text
    let n = 0; const vt = layer.readPointer();
    while (n < 512) { const e = vt.add(n * 8).readPointer(); if (!inBee(e)) break; n++; }
    o.vtable_len_guess = n;
  } catch (e) { o.err = '' + e; }
  log(o);
  hookVslot(layer, 79, 's79');
  hookVslot(layer, 183, 's183');
  hookVslot(layer, 184, 's184');
  hookVslot(layer, 56, 's56');
  hookVslot(layer, 65, 's65');
}

function installBee() {
  const exp = (n) => bee.getExportByName(n);
  // BEE_GetSourceTimeFormat(BEE_Layer const*, bool, int*, T_TimeFormatInfo*)
  Interceptor.attach(exp('?BEE_GetSourceTimeFormat@@YAHPEBVBEE_Layer@@_NPEAHPEAUT_TimeFormatInfo@@@Z'), {
    onEnter(args) {
      this.hit = inTimecode(this.returnAddress);
      if (!this.hit) return;
      this.layer = args[0]; this.flag = args[1].toInt32(); this.fps = args[2]; this.fmt = args[3];
      log({ ev: 'BEE_GetSourceTimeFormat.enter', layer: hex(this.layer), flag: this.flag, fps_in: this.fps.readS32(), fmt_in: safeBytes(this.fmt, 0x18) });
      dumpLayer(this.layer, 'GetSourceTimeFormat');
      try { const it = this.layer.add(0x260).readPointer(); if (!it.isNull()) { dumpItem(it, 'layer+0x260'); dumpProject(it.add(0x38).readPointer(), 'item+0x38'); } } catch (e) { log({ ev: 'err', e: '' + e }); }
    },
    onLeave(rv) {
      if (!this.hit) return;
      log({ ev: 'BEE_GetSourceTimeFormat.leave', ret: rv.toInt32(), fps_out: this.fps.readS32(), fmt_out: safeBytes(this.fmt, 0x18) });
    }
  });
  Interceptor.attach(exp('?BEE_LayerToSourceTime@@YAHPEBVBEE_Layer@@PEBUT_Time@@PEAU2@PEBVTDB_ParamBag@@@Z'), {
    onEnter(args) {
      this.hit = inTimecode(this.returnAddress);
      if (!this.hit) return;
      this.out = args[2];
      log({ ev: 'BEE_LayerToSourceTime.enter', layer: hex(args[0]), t_in: safeBytes(args[1], 8), bag: hex(args[3]) });
    },
    onLeave(rv) { if (this.hit) log({ ev: 'BEE_LayerToSourceTime.leave', ret: rv.toInt32(), t_out: safeBytes(this.out, 8) }); }
  });
  Interceptor.attach(exp('?BEE_GetProjectTimeFormat@@YAXPEBVBEE_Project@@AEAUT_TimeFormatInfo@@@Z'), {
    onEnter(args) {
      this.hit = inTimecode(this.returnAddress);
      if (!this.hit) return;
      this.fmt = args[1];
      log({ ev: 'BEE_GetProjectTimeFormat.enter', project: hex(args[0]) });
      dumpProject(args[0], 'GetProjectTimeFormat');
    },
    onLeave(rv) { if (this.hit) log({ ev: 'BEE_GetProjectTimeFormat.leave', fmt_out: safeBytes(this.fmt, 0x18) }); }
  });
  Interceptor.attach(exp('?GetParentProject@BEE_Item@@QEAAAEAPEAVBEE_Project@@XZ'), {
    onEnter(args) { this.hit = inTimecode(this.returnAddress); if (this.hit) { this.item = args[0]; } },
    onLeave(rv) { if (this.hit) { log({ ev: 'BEE_Item::GetParentProject', item: hex(this.item), ret_ref: hex(rv), project: hex(rv.readPointer()) }); dumpItem(this.item, 'GetParentProject'); } }
  });
  Interceptor.attach(exp('?GetFlags@BEE_Item@@QEAAAEAU?$atomic@H@std@@XZ'), {
    onEnter(args) { this.hit = inTimecode(this.returnAddress); if (this.hit) { this.item = args[0]; } },
    onLeave(rv) { if (this.hit) { log({ ev: 'BEE_Item::GetFlags', item: hex(this.item), flags: '0x' + rv.readU32().toString(16) }); dumpItem(this.item, 'GetFlags'); } }
  });
  Interceptor.attach(exp('?BEE_GetSourceMediaInfo@@YAHPEBVBEE_Item@@PEAVBEE_SourceMediaInfo@@PEA_N@Z'), {
    onEnter(args) {
      this.hit = inRender && (inBee(this.returnAddress) || inTimecode(this.returnAddress));
      if (!this.hit) return; this.item = args[0]; this.info = args[1]; this.has = args[2];
      log({ ev: 'BEE_GetSourceMediaInfo.enter', item: hex(this.item), from: modOff(this.returnAddress) });
    },
    onLeave(rv) { if (this.hit) log({ ev: 'BEE_GetSourceMediaInfo.leave', ret: rv.toInt32(), has: this.has.readU8(), info: safeBytes(this.info, 0x40) }); }
  });
  Interceptor.attach(exp('?BEE_GetCompSettings@@YAHPEAVBEE_CompItem@@PEAUBEE_CompSettings@@@Z'), {
    onEnter(args) { this.hit = inRender && inBee(this.returnAddress); if (this.hit) { this.cs = args[1]; this.item = args[0]; } },
    onLeave(rv) { if (this.hit) log({ ev: 'BEE_GetCompSettings.leave', item: hex(this.item), ret: rv.toInt32(), cs: safeBytes(this.cs, 0x50), from: 'bee' }); }
  });
  // internal fps getter FUN_180475aa0
  Interceptor.attach(bee.base.add(0x475aa0), {
    onEnter(args) { this.item = args[0]; },
    onLeave(rv) { if (inRender && inBee(this.returnAddress)) log({ ev: 'BEE.fps_getter_475aa0', item: hex(this.item), ret: '0x' + rv.toString(16) }); }
  });
  log({ ev: 'bee_hooked', base: hex(bee.base) });
}
function installU() {
  const exp = (n) => u.getExportByName(n);
  Interceptor.attach(exp('?T_GeneralFormatTime@@YAXPEBUT_Time@@PEBUT_TimeFormatInfo@@W4T_TimeFormatType@@H_NPEAD_K@Z'), {
    onEnter(args) {
      this.hit = inTimecode(this.returnAddress);
      if (!this.hit) return;
      this.buf = args[5];
      log({ ev: 'T_GeneralFormatTime.enter', t: safeBytes(args[0], 8), fmt: safeBytes(args[1], 0x18), type: args[2].toInt32(), fps: args[3].toInt32(), flag: args[4].toInt32(), size: args[6].toInt32() });
    },
    onLeave(rv) { if (this.hit) log({ ev: 'T_GeneralFormatTime.leave', str: this.buf.readCString() }); }
  });
  Interceptor.attach(exp('?T_FrameRate2Duration@@YAHHPEAUT_Time@@@Z'), {
    onEnter(args) { this.hit = inTimecode(this.returnAddress); this.fps = args[0].toInt32(); this.out = args[1]; },
    onLeave(rv) { if (this.hit) log({ ev: 'T_FrameRate2Duration', fps: this.fps, ret: rv.toInt32(), out: safeBytes(this.out, 8) }); }
  });
  Interceptor.attach(exp('?T_GetMaxFPS@@YAHXZ'), {
    onEnter(args) { this.hit = inTimecode(this.returnAddress) || (u && this.returnAddress.compare(u.base) >= 0 && this.returnAddress.compare(u.base.add(u.size)) < 0); },
    onLeave(rv) { if (this.hit) log({ ev: 'T_GetMaxFPS', ret: rv.toInt32() }); }
  });
  log({ ev: 'u_hooked', base: hex(u.base) });
}
function installTimecode() {
  Interceptor.attach(tc.base.add(0x61a0), {
    onEnter(args) {
      this.in_data = args[0]; this.params = args[2]; this.output = args[3];
      inRender = true;
      const o = { ev: 'RENDER.enter', in_data: hex(args[0]), params: hex(args[2]), output: hex(args[3]) };
      try {
        o.current_time = args[0].add(0xe0).readS32(); o.time_scale = args[0].add(0xf0).readU32(); o.time_step = args[0].add(0xe8).readS32();
        const pv = [];
        for (let i = 0; i < 16; i++) { try { const p = args[2].add(i * 8).readPointer(); pv.push(safeBytes(p.add(0x38), 8)); } catch (e) { pv.push('?'); } }
        o.param_values_0x38 = pv;
      } catch (e) { o.err = '' + e; }
      log(o);
    },
    onLeave(rv) { inRender = false; log({ ev: 'RENDER.leave', ret: rv.toInt32() }); }
  });
  log({ ev: 'timecode_hooked', base: hex(tc.base), size: tc.size });
}

function tryInstall() {
  if (!tc) { tc = Process.findModuleByName('Timecode.aex'); if (tc) installTimecode(); }
  if (!bee) { bee = Process.findModuleByName('BEE.dll'); if (bee) installBee(); }
  if (!u) { u = Process.findModuleByName('U.dll'); if (u) installU(); }
  return tc && bee && u;
}
if (!tryInstall()) {
  const ntdll = Process.getModuleByName('ntdll.dll');
  Interceptor.attach(ntdll.getExportByName('LdrLoadDll'), {
    onLeave(rv) { if (!(tc && bee && u)) tryInstall(); }
  });
  const iv = setInterval(() => { if (tryInstall()) clearInterval(iv); }, 200);
}
log({ ev: 'script_loaded', pid: Process.id });
