#pragma once

// The route this process serves. It is a runtime value rather than a compiler
// definition so the whole runtime links as one macro-neutral object graph
// (issue #1495): there is a single worker executable and a single link step,
// which is what makes a half-stale build impossible.
//
// `Discovery` was called L2 until #1495. That came from an L0/L1/L2 staging
// plan whose L1 (load, dispatch nothing, unload) was deleted in #732, leaving
// a number that no longer distinguished anything. What the route actually does
// is load the plug-in, run About/GlobalSetup/ParamsSetup, the sequence and
// frame setup/setdown pair and GlobalSetdown, observe the parameters, and
// render nothing.
//
// `Classic` was called Render. Both render, so the old name did not separate
// them; Classic is the term the docs and the corpus already use for the
// non-SmartFX selector contract.
namespace aexcompat::worker_target {

enum class Kind { Discovery, Classic, Smart };

int run(Kind kind, int argc, wchar_t** argv);

// Parses a leading `--kind <discovery|classic|smart>` pair. Deliberately not
// one of the auxiliary options: those are stripped from the tail by
// `strip_auxiliary_options` after the runtime already knows its kind, and the
// positional contract (argv[1] onwards: command, plug-in, sha256, ...) must
// keep its indices. Consuming the pair up front and handing the runtime the
// remaining vector leaves that contract untouched.
//
// Fails closed. There is no default kind: before #1495 the selector defaulted
// to L2, so a caller that forgot to set it silently got discovery instead of
// the render route it asked for.
bool parse_kind(const wchar_t* value, Kind& kind);

}  // namespace aexcompat::worker_target
