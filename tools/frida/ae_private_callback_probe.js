'use strict';

// After Effects private get_callback_addr probe (issue #985).
//
// Loaded into a running AfterFX host by tools/capture_ae_private_callbacks.py.
// It hooks the entry point of each target effect plug-in module as it loads,
// reads `in_data->utils->get_callback_addr` out of the live PF_InData the host
// hands the plug-in, hooks that host function, and reports every request
// (which id / quality / mode) with the module + RVA of the function AE returns.
// On the first answered request it also enumerates the dispatcher for a range
// of ids and both quality values / three mode values, and probes the returned
// functions:
//   * id -5 (double(double)) is called with a table of inputs;
//   * id -2 (the in-place blur) is called on synthetic 8-bit / 16-bit worlds
//     (impulse, half-alpha impulse, step, ramp) built by hand with the
//     PF_LayerDef layout, for a set of radii / flags / in_data->quality
//     values, and the resulting pixels are sent back as binary payloads.
// Every id -2 call the plug-ins themselves make is recorded with its radius
// (read from the callee's spill slot), flags, in_data->quality and the world's
// geometry, plus the world pixels before and after the call.
//
// Nothing here writes files; all output goes through send(). Module bases and
// pointers are reported as module name + RVA (ASLR-independent), never as raw
// addresses in the persisted events (the driver keeps what it is given).

const TARGETS = (function () {
  const spec = 'AEXCAP_TARGETS_PLACEHOLDER';
  return spec.split(';').map(s => s.trim()).filter(s => s.length > 0);
})();

// PF_InData / PF_UtilCallbacks / PF_LayerDef offsets on x64 (the generated
// contract in minihost/src/generated/aex_abi_contract.hpp).
const IN_UTILS_OFFSET = 176;
const IN_EFFECT_REF_OFFSET = 184;
const IN_QUALITY_OFFSET = 192;
const UTILS_GET_CALLBACK_ADDR_OFFSET = 192;
const LAYER_FLAGS_OFFSET = 0x10;
const LAYER_DATA_OFFSET = 0x18;
const LAYER_ROWBYTES_OFFSET = 0x20;
const LAYER_WIDTH_OFFSET = 0x24;
const LAYER_HEIGHT_OFFSET = 0x28;
const LAYER_EXTENT_OFFSET = 0x2c;

const hookedModules = new Set();
const hookedGca = new Set();
const hookedReturned = new Map();
const callCounts = {};
let lastGca = null;
let lastRadius = null;
let m2Index = 0;
let m2Probed = false;
let dispatcherEnumerated = false;

function log(o) { send(o); }

function modInfo(p) {
  const m = Process.findModuleByAddress(p);
  if (!m) return { module: null, rva: null };
  return { module: m.name, rva: '0x' + p.sub(m.base).toString(16) };
}

function hexbytes(p, n) {
  try {
    const u = new Uint8Array(p.readByteArray(n));
    let s = '';
    for (let i = 0; i < u.length; i++) s += ('0' + u[i].toString(16)).slice(-2);
    return s;
  } catch (e) { return 'unreadable'; }
}

function countCall(key, tag) {
  const k = tag + ':' + key;
  callCounts[k] = (callCounts[k] || 0) + 1;
  return callCounts[k];
}

function backtrace(context, depth) {
  return Thread.backtrace(context, Backtracer.ACCURATE).slice(0, depth).map(a => {
    const mi = modInfo(a);
    return (mi.module || '?') + '+' + (mi.rva || '?');
  });
}

// ---- probes of the returned functions --------------------------------------

function probeGaussianValue(fn, plugin) {
  try {
    const f = new NativeFunction(fn, 'double', ['double']);
    const xs = [-1.0, -0.5, -0.1, 0.0, 0.05, 0.1, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 0.95, 1.0, 1.1, 1.5, 2.0];
    log({ ev: 'm5_probe', plugin, fn: modInfo(fn), table: xs.map(x => [x, f(x)]) });
  } catch (e) { log({ ev: 'm5_probe_failed', plugin, err: String(e) }); }
}

function enumerateDispatcher(gca, effectRef) {
  try {
    const f = new NativeFunction(gca, 'int', ['pointer', 'int', 'uint', 'int', 'pointer']);
    const out = Memory.alloc(8);
    const table = [];
    for (let q = 0; q <= 1; q++) for (let m = 0; m <= 2; m++) for (let id = -12; id <= 40; id++) {
      out.writePointer(NULL);
      const err = f(effectRef, q, m, id, out);
      const p = out.readPointer();
      table.push({ q, m, id, err, fn: p.isNull() ? null : modInfo(p) });
    }
    log({ ev: 'dispatcher_table', table });
  } catch (e) { log({ ev: 'dispatcher_enum_failed', err: String(e) }); }
}

function probeBlur(gca, inData) {
  try {
    const effectRef = inData.add(IN_EFFECT_REF_OFFSET).readPointer();
    const f = new NativeFunction(gca, 'int', ['pointer', 'int', 'uint', 'int', 'pointer']);
    const out = Memory.alloc(8);
    const fns = {};
    for (const m of [0, 1]) {
      out.writePointer(NULL);
      const e = f(effectRef, 1, m, -2, out);
      fns[m] = out.readPointer();
      log({ ev: 'm2_probe_fn', m, err: e, fn: modInfo(fns[m]) });
    }
    const savedQuality = inData.add(IN_QUALITY_OFFSET).readS32();
    const W = 65, H = 65;
    const world = Memory.alloc(0x80);
    const pixels = Memory.alloc(W * H * 8);
    const progress = Memory.alloc(16);
    // deep 0 = 8-bit ARGB, 1 = 16-bit ARGB (PFp_WorldDepth reads the byte at
    // world_flags + 3, i.e. bits 24..31: 1 -> 16-bit; 2 -> 32f, which this
    // probe does not build - a hand-made float world made AE throw).
    function setupWorld(kind, deep) {
      const bpc = deep ? 2 : 1, RB = W * 4 * bpc, MAX = deep ? 32768 : 255;
      world.writeByteArray(new Array(0x80).fill(0));
      world.add(LAYER_FLAGS_OFFSET).writeU32(deep ? ((1 << 24) | 3) : 2);
      world.add(LAYER_DATA_OFFSET).writePointer(pixels);
      world.add(LAYER_ROWBYTES_OFFSET).writeS32(RB);
      world.add(LAYER_WIDTH_OFFSET).writeS32(W);
      world.add(LAYER_HEIGHT_OFFSET).writeS32(H);
      world.add(LAYER_EXTENT_OFFSET).writeS32(0);
      world.add(LAYER_EXTENT_OFFSET + 4).writeS32(0);
      world.add(LAYER_EXTENT_OFFSET + 8).writeS32(W);
      world.add(LAYER_EXTENT_OFFSET + 12).writeS32(H);
      const buf = deep ? new Uint16Array(W * H * 4) : new Uint8Array(W * H * 4);
      const set = (x, y, a, r, g, b) => { const o = (y * W + x) * 4; buf[o] = a; buf[o + 1] = r; buf[o + 2] = g; buf[o + 3] = b; };
      if (kind === 'impulse') {
        set(32, 32, MAX, MAX, MAX, MAX);
      } else if (kind === 'halfalpha') {
        set(32, 32, MAX >> 1, Math.round(MAX * 0.8), Math.round(MAX * 0.4), 0);
      } else if (kind === 'step') {
        for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
          const on = (x < 32) && (y < 32);
          set(x, y, on ? MAX : 0, on ? Math.round(MAX * 200 / 255) : 0, on ? Math.round(MAX * 100 / 255) : 0, on ? Math.round(MAX * 50 / 255) : 0);
        }
      } else if (kind === 'ramp') {
        for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
          set(x, y, MAX, Math.round(x * 3 * MAX / 255), Math.round(y * 3 * MAX / 255), Math.round(((x * 7 + y * 13) & 255) * MAX / 255));
        }
      }
      pixels.writeByteArray(buf.buffer);
      return RB;
    }
    const radii = [0.1, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 1.9, 2.0, 2.25, 2.5, 2.71, 2.72, 2.75, 3.0, 3.5, 3.7, 4.0, 5.0, 6.3, 8.0, 10.0, 12.5, 15.5, 20.0];
    const cases = [];
    for (const deep of [0, 1]) {
      for (const q of [1, 0]) for (const r of radii) {
        cases.push({ kind: 'impulse', deep, m: 0, q, r, flags: 0x4f });
        if (!deep) cases.push({ kind: 'impulse', deep, m: 0, q, r, flags: 0x6f });
      }
      for (const r of [1.0, 2.0, 3.7, 6.3, 10.0]) {
        cases.push({ kind: 'impulse', deep, m: 1, q: 1, r, flags: 0x4f });
        cases.push({ kind: 'halfalpha', deep, m: 1, q: 1, r, flags: 0x4f });
        cases.push({ kind: 'halfalpha', deep, m: 0, q: 1, r, flags: 0x4f });
        cases.push({ kind: 'halfalpha', deep, m: 1, q: 1, r, flags: 0x41 });
        cases.push({ kind: 'halfalpha', deep, m: 1, q: 1, r, flags: 0x4e });
        cases.push({ kind: 'step', deep, m: 1, q: 1, r, flags: 0x7f });
        cases.push({ kind: 'step', deep, m: 1, q: 1, r, flags: 0x6f });
        cases.push({ kind: 'step', deep, m: 0, q: 1, r, flags: 0x7f });
        cases.push({ kind: 'step', deep, m: 0, q: 1, r, flags: 0x6f });
        cases.push({ kind: 'step', deep, m: 1, q: 0, r, flags: 0x7f });
        cases.push({ kind: 'step', deep, m: 1, q: 1, r, flags: 0x71 });
        cases.push({ kind: 'ramp', deep, m: 1, q: 1, r, flags: 0x7f });
        cases.push({ kind: 'ramp', deep, m: 1, q: 1, r, flags: 0x6f });
        cases.push({ kind: 'ramp', deep, m: 0, q: 1, r, flags: 0x6f });
        cases.push({ kind: 'ramp', deep, m: 1, q: 1, r, flags: 0x16f });
      }
    }
    let idx = 0;
    for (const c of cases) {
      const RB = setupWorld(c.kind, c.deep);
      inData.add(IN_QUALITY_OFFSET).writeS32(c.q);
      progress.writeS32(0);
      const fn = new NativeFunction(fns[c.m], 'int', ['pointer', 'pointer', 'double', 'pointer', 'int', 'pointer']);
      const err = fn(inData, NULL, c.r, progress, c.flags, world);
      send({ ev: 'm2_probe_case', idx, kind: c.kind, deep: c.deep, m: c.m, q: c.q, r: c.r, flags: c.flags, err, W, H, RB,
             progress: progress.readS32() }, pixels.readByteArray(RB * H));
      idx++;
    }
    inData.add(IN_QUALITY_OFFSET).writeS32(savedQuality);
    log({ ev: 'm2_probe_done', cases: cases.length });
  } catch (e) { log({ ev: 'm2_probe_failed', err: String(e) }); }
}

// ---- hooks -------------------------------------------------------------------

function hookReturned(fn, id, quality, mode, plugin) {
  const key = fn.toString();
  if (hookedReturned.has(key)) return;
  hookedReturned.set(key, { id, quality, mode });
  log({ ev: 'returned_fn', plugin, id, quality, mode, fn: modInfo(fn), code_prefix: hexbytes(fn, 96) });
  if (id !== -2) return;
  // Read the double radius (xmm2) after the callee spills it to [rsp+0x58]:
  // both FLT.dll entries share the same 0x3f-byte prologue ending in
  // `vmovsd [rsp+0x58], xmm2`. Anything else is reported, not assumed.
  try {
    const pro = hexbytes(fn, 0x3f);
    if (pro.startsWith('4c894c2420535657') && pro.endsWith('c5fb11542458')) {
      Interceptor.attach(fn.add(0x3f), { onEnter() { lastRadius = this.context.rsp.add(0x58).readDouble(); } });
    } else {
      log({ ev: 'm2_prologue_mismatch', prologue: pro });
    }
  } catch (e) { log({ ev: 'm2_radius_hook_failed', err: String(e) }); }
  try {
    Interceptor.attach(fn, {
      onEnter(args) {
        const n = countCall(key, 'm2');
        if (n > 40) return;
        this.rec = true;
        this.args = [args[0], args[1], args[2], args[3], args[4], args[5]];
        this.quality_in = args[0].add(IN_QUALITY_OFFSET).readS32();
        const w = args[5];
        try {
          const width = w.add(LAYER_WIDTH_OFFSET).readS32(), height = w.add(LAYER_HEIGHT_OFFSET).readS32();
          const rowbytes = w.add(LAYER_ROWBYTES_OFFSET).readS32();
          const data = w.add(LAYER_DATA_OFFSET).readPointer();
          this.world = { flags: w.add(LAYER_FLAGS_OFFSET).readU32(), width, height, rowbytes,
            extent: [0, 4, 8, 12].map(o => w.add(LAYER_EXTENT_OFFSET + o).readS32()) };
          if (height > 0 && height <= 4096 && rowbytes > 0 && rowbytes <= 65536) {
            this.pixBefore = data.readByteArray(rowbytes * height);
            this.pixLen = rowbytes * height;
            this.pixData = data;
          }
        } catch (e) { this.world = { err: String(e) }; }
        this.bt = backtrace(this.context, 4);
      },
      onLeave(retval) {
        if (!this.rec) return;
        const idx = m2Index++;
        let pixAfter = null;
        if (this.pixData) { try { pixAfter = this.pixData.readByteArray(this.pixLen); } catch (e) {} }
        log({ ev: 'm2_call', idx, retval: retval.toInt32(), bt: this.bt, quality_in: this.quality_in,
              radius: lastRadius, flags: this.args[4].toInt32(), world: this.world });
        if (!m2Probed && lastGca) { m2Probed = true; probeBlur(lastGca, this.args[0]); }
        if (this.pixBefore) send({ ev: 'm2_pixels', idx, which: 'before', len: this.pixLen }, this.pixBefore);
        if (pixAfter) send({ ev: 'm2_pixels', idx, which: 'after', len: this.pixLen }, pixAfter);
      }
    });
  } catch (e) { log({ ev: 'hook_returned_failed', id, err: String(e) }); }
}

function hookGca(gca, plugin) {
  const key = gca.toString();
  if (hookedGca.has(key)) return;
  hookedGca.add(key);
  lastGca = gca;
  log({ ev: 'gca_found', plugin, gca: modInfo(gca), code_prefix: hexbytes(gca, 32) });
  Interceptor.attach(gca, {
    onEnter(args) {
      this.effect_ref = args[0];
      this.quality = args[1].toInt32();
      this.mode = args[2].toUInt32();
      this.id = args[3].toInt32();
      this.out = args[4];
      this.bt = backtrace(this.context, 4);
    },
    onLeave(retval) {
      let fn = null;
      try { fn = this.out.isNull() ? null : this.out.readPointer(); } catch (e) { fn = null; }
      const n = countCall(key, 'gca:' + this.id);
      if (n <= 3) log({ ev: 'gca_call', id: this.id, quality: this.quality, mode: this.mode, err: retval.toInt32(),
                        fn: fn ? modInfo(fn) : null, bt: this.bt, n });
      if (fn && !fn.isNull() && retval.toInt32() === 0) {
        hookReturned(fn, this.id, this.quality, this.mode, this.bt[0] || '?');
        if (this.id === -5 && n === 1) probeGaussianValue(fn, this.bt[0] || '?');
      }
      if (!dispatcherEnumerated) { dispatcherEnumerated = true; enumerateDispatcher(gca, this.effect_ref); }
    }
  });
}

function hookPlugin(mod) {
  if (hookedModules.has(mod.name)) return;
  hookedModules.add(mod.name);
  const exps = mod.enumerateExports().filter(e => e.type === 'function');
  log({ ev: 'module', name: mod.name, exports: exps.map(e => e.name) });
  // The PiPL entry point name varies (EffectMain / EffectMainExtra / MainEntry /
  // FilterMain); the registration export is PluginDataEntryFunction*.
  const entry = exps.find(e => e.name === 'EffectMain') ||
      exps.find(e => /main/i.test(e.name) && !/PluginData/i.test(e.name)) || exps[0];
  if (!entry) { log({ ev: 'no_entry', name: mod.name }); return; }
  Interceptor.attach(entry.address, {
    onEnter(args) {
      const cmd = args[0].toInt32();
      const inData = args[1];
      const n = countCall(mod.name, 'cmd:' + cmd);
      if (n <= 2) log({ ev: 'effect_main', plugin: mod.name, cmd, n });
      if (inData.isNull()) return;
      try {
        const utils = inData.add(IN_UTILS_OFFSET).readPointer();
        if (utils.isNull()) return;
        const gca = utils.add(UTILS_GET_CALLBACK_ADDR_OFFSET).readPointer();
        if (!gca.isNull()) hookGca(gca, mod.name);
      } catch (e) { log({ ev: 'read_utils_failed', plugin: mod.name, err: String(e) }); }
    }
  });
  log({ ev: 'hooked', name: mod.name, entry: entry.name });
}

function scan() {
  for (const t of TARGETS) {
    if (hookedModules.has(t)) continue;
    const m = Process.findModuleByName(t);
    if (m) hookPlugin(m);
  }
}
scan();
setInterval(scan, 200);
// Effect plug-ins load lazily (on first use), so rescan right after any DLL load.
try {
  const ntdll = Process.getModuleByName('ntdll.dll');
  Interceptor.attach(ntdll.getExportByName('LdrLoadDll'), { onLeave() { scan(); } });
} catch (e) { log({ ev: 'ldr_hook_failed', err: String(e) }); }
log({ ev: 'ready', targets: TARGETS });
