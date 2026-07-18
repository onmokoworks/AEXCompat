'use strict';

// Thin known-function observer. All interpretation planning happens in Python
// (tools/known_function_observation.py); this script only reads the scalars a
// resolved read plan names and forwards them with send(). It never writes
// files, never stops execution (send() is asynchronous, so a 30s worker render
// timeout is unaffected), and never forwards raw pointers or bytes: struct
// fields and register scalars are reduced to numbers/booleans before sending.
//
// Protocol: the launcher posts {type:'plan', plan, module_file} once. The plan
// is the output of resolve_spec(); module_file is the on-disk basename used
// only to locate the module base (it is not logged).

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
// value, not an address; only genuine value args should be declared as
// scalar_args in the hook spec.
function readRegister(slot, read) {
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

function collect(reads, args) {
  const fields = [];
  for (const read of reads) {
    const slot = args[read.arg_index];
    const value = read.source === 'struct' ? readStruct(slot, read) : readRegister(slot, read);
    fields.push({ name: read.name, value: value });
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

function attachHook(base, hook) {
  const slotCount = maxArgIndex(hook) + 1;
  Interceptor.attach(base.add(hook.module_rva_int), {
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
        fields: collect(hook.enter_reads, this.slots),
      });
    },
    onLeave: function (retval) {
      const message = {
        type: 'known_function',
        symbol: hook.symbol,
        module_rva: hook.module_rva,
        phase: 'leave',
        fields: collect(hook.leave_reads, this.slots),
      };
      if (hook.return) {
        message.return_value = interpretReturn(retval, hook.return);
      }
      send(message);
    },
  });
}

function tryAttach(plan, moduleFile) {
  const module = Process.findModuleByName(moduleFile);
  if (module === null) return false;
  for (const hook of plan.hooks) {
    attachHook(module.base, hook);
  }
  return true;
}

// When the launcher spawns the worker suspended, the plug-in DLL is not mapped
// yet — it is LoadLibrary'd later, during the render. So we cannot install the
// hooks before resume. Instead we arm a loader watch synchronously (before the
// launcher resumes) and attach the moment the module appears. The known
// functions are render-path functions invoked well after load, so attaching in
// LoadLibrary's onLeave (right after the DLL is mapped) never misses them.
function installPlan(plan, moduleFile) {
  if (tryAttach(plan, moduleFile)) {
    send({ type: 'ready', installed: true, hook_count: plan.hooks.length });
    return;
  }
  const k32 = Process.getModuleByName('kernel32.dll');
  let attached = false;
  const watch = function () {
    if (attached) return;
    if (tryAttach(plan, moduleFile)) {
      attached = true;
      send({ type: 'installed', hook_count: plan.hooks.length });
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
    installPlan(message.plan, message.module_file);
  } catch (err) {
    send({ type: 'install_error', message: String(err && err.message ? err.message : err) });
  }
});
