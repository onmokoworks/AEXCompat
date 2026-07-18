'use strict';

// Thin known-function observer. All interpretation planning happens in Python
// (tools/known_function_observation.py); this script only reads the scalars a
// resolved read plan names and forwards them with send(). It never writes files,
// never stops execution (send() is asynchronous, so a 30s worker render timeout
// is unaffected), and never forwards raw pointers or bytes: struct fields and
// register scalars are reduced to numbers/booleans before sending.
//
// Safety, in addition to the Python-side plan validation:
//   * module identity is matched by full canonical path, not just basename, so a
//     same-named dependency DLL or a swapped module is not hooked;
//   * every hook target is verified to fall inside the module image and in an
//     executable range before Interceptor.attach (fail-closed otherwise);
//   * every read null-checks its pointer and stays within the declared struct
//     extent; a read failure is reported as an observation error, never thrown
//     out of the Frida callback (which could destabilise the worker).
//
// Protocol: the launcher posts {type:'plan', plan, module_file, module_path}
// once. module_path is the expected canonical path used only to bind identity
// and locate the base; it is not logged.

function normalizePath(p) {
  return String(p).replace(/\//g, '\\').toLowerCase();
}

function readStruct(base, read) {
  const p = base.add(read.offset);
  switch (read.interpret) {
    case 'int':
      if (read.size === 1) return p.readS8();
      if (read.size === 2) return p.readS16();
      if (read.size === 4) return p.readS32();
      return p.readS64().toNumber();
    case 'uint':
      if (read.size === 1) return p.readU8();
      if (read.size === 2) return p.readU16();
      if (read.size === 4) return p.readU32();
      return p.readU64().toNumber();
    case 'float':
      return read.size === 4 ? p.readFloat() : p.readDouble();
    case 'bool':
      return p.readU8() !== 0;
    default:
      throw new Error('unknown interpret ' + read.interpret);
  }
}

// Register scalars arrive as NativePointer-typed argument slots but hold a
// value, not an address; only genuine value args are declared as scalar_args
// (pointers are not representable in the schema). Read exactly the declared
// width.
function readRegister(slot, read) {
  if (read.width === 8) {
    const n = parseInt(slot.toString(), 16);
    return read.interpret === 'bool' ? n !== 0 : n;
  }
  switch (read.interpret) {
    case 'uint':
      return slot.toUInt32();
    case 'bool':
      return slot.toInt32() !== 0;
    case 'int':
    default:
      return slot.toInt32();
  }
}

function readOne(read, slot) {
  if (slot === undefined || slot === null) {
    throw new Error('absent arg slot for ' + read.name);
  }
  if (read.source === 'struct') {
    // A null struct pointer is a genuine failure; a register scalar of 0 is a
    // legitimate value (NativePointer(0)), so only struct reads reject null.
    if (slot.isNull()) {
      throw new Error('null struct pointer for ' + read.name);
    }
    // Defence in depth: the resolver already bounds offset/size within extent,
    // but re-check at runtime so a malformed plan cannot walk outside the struct.
    if (read.offset < 0 || read.offset + read.size > read.extent) {
      throw new Error('read out of struct extent for ' + read.name);
    }
    return readStruct(slot, read);
  }
  return readRegister(slot, read);
}

function collect(reads, slots, ctx) {
  const fields = [];
  for (const read of reads) {
    try {
      fields.push({ name: read.name, value: readOne(read, slots[read.arg_index]) });
    } catch (err) {
      // Never throw out of the callback; report and omit the field.
      send({ type: 'read_error', symbol: ctx.symbol, phase: ctx.phase, name: read.name,
             message: String(err && err.message ? err.message : err) });
    }
  }
  return fields;
}

function interpretReturn(retval, spec) {
  return spec.interpret === 'uint' ? retval.toUInt32() : retval.toInt32();
}

function maxArgIndex(hook) {
  let max = -1;
  for (const read of hook.enter_reads.concat(hook.leave_reads)) {
    if (read.arg_index > max) max = read.arg_index;
  }
  return max;
}

// Verify a hook target is inside the module image and executable. Throws
// (fail-closed) so the caller aborts before attaching anything.
function verifyTarget(module, hook) {
  const rva = hook.module_rva_int;
  if (rva < 0 || rva >= module.size) {
    throw new Error('rva 0x' + rva.toString(16) + ' is outside module image size ' + module.size);
  }
  const target = module.base.add(rva);
  const range = Process.findRangeByAddress(target);
  if (range === null || range.protection.indexOf('x') === -1) {
    throw new Error('rva 0x' + rva.toString(16) + ' is not in an executable range');
  }
  return target;
}

function attachHook(module, hook) {
  const target = verifyTarget(module, hook);
  const slotCount = maxArgIndex(hook) + 1;
  Interceptor.attach(target, {
    onEnter: function (args) {
      // Snapshot the argument slots the plan names so onLeave can re-read struct
      // outputs (e.g. out.* fields the call writes) from the same pointers.
      this.slots = [];
      for (let i = 0; i < slotCount; i++) this.slots.push(args[i]);
      send({
        type: 'known_function',
        symbol: hook.symbol,
        module_rva: hook.module_rva,
        phase: 'enter',
        fields: collect(hook.enter_reads, this.slots, { symbol: hook.symbol, phase: 'enter' }),
      });
    },
    onLeave: function (retval) {
      const message = {
        type: 'known_function',
        symbol: hook.symbol,
        module_rva: hook.module_rva,
        phase: 'leave',
        fields: collect(hook.leave_reads, this.slots, { symbol: hook.symbol, phase: 'leave' }),
      };
      if (hook.return) {
        message.return_value = interpretReturn(retval, hook.return);
      }
      send(message);
    },
  });
}

// Locate the target module by its full canonical path, not by basename, so a
// same-named dependency or a swapped module is not mistaken for the plug-in the
// worker admitted.
function findTargetModule(modulePath) {
  const want = normalizePath(modulePath);
  const modules = Process.enumerateModules();
  for (const module of modules) {
    if (normalizePath(module.path) === want) return module;
  }
  return null;
}

// Validate every target first, then attach, so a single out-of-range hook fails
// the whole install closed rather than attaching a partial, inconsistent set.
function attachAll(module, plan) {
  for (const hook of plan.hooks) verifyTarget(module, hook);
  for (const hook of plan.hooks) attachHook(module, hook);
}

function tryAttach(plan, modulePath) {
  const module = findTargetModule(modulePath);
  if (module === null) return false;
  attachAll(module, plan);
  return true;
}

// When the launcher spawns the worker suspended, the plug-in DLL is not mapped
// yet - it is LoadLibrary'd later, during the render. So we cannot attach the
// hooks before resume. Instead we arm a loader watch synchronously (before the
// launcher resumes) and attach the moment the identity-matched module appears.
// The known functions are render-path functions invoked well after load, so
// attaching in LoadLibrary's onLeave (right after the DLL is mapped) never
// misses them.
function installPlan(plan, modulePath) {
  if (tryAttach(plan, modulePath)) {
    send({ type: 'ready', installed: true, hook_count: plan.hooks.length });
    return;
  }
  const k32 = Process.getModuleByName('kernel32.dll');
  let attached = false;
  const watch = function () {
    if (attached) return;
    try {
      if (tryAttach(plan, modulePath)) {
        attached = true;
        send({ type: 'installed', hook_count: plan.hooks.length });
      }
    } catch (err) {
      attached = true; // stop retrying; surface the failure
      send({ type: 'install_error', message: String(err && err.message ? err.message : err) });
    }
  };
  for (const name of ['LoadLibraryW', 'LoadLibraryExW', 'LoadLibraryA', 'LoadLibraryExA']) {
    const addr = k32.findExportByName(name);
    if (addr !== null) Interceptor.attach(addr, { onLeave: watch });
  }
  // 'ready' means the loader watch is armed; it is now safe for the launcher to
  // resume. The later 'installed' message reports the actual attach.
  send({ type: 'ready', installed: false, hook_count: plan.hooks.length });
}

recv('plan', function onPlan(message) {
  try {
    installPlan(message.plan, message.module_path);
  } catch (err) {
    send({ type: 'install_error', message: String(err && err.message ? err.message : err) });
  }
});
