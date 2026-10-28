# Numeric declared defaults

`InteractiveParameter.use_declared_default` (serde default `false`) selects the
numeric default declared by the **loaded plug-in**. Native image and interactive
frame APIs share this transport. The caller's `value` is ignored in this mode;
it is not a range-check exemption for a supplied number.

The worker payload is `v7|param_5@5:f64=default` (or `i32=default` for an integer,
popup, checkbox or path). v7 also accepts the older typed assignments. The
worker validates the actual slot/type and resolves the default on every apply,
so a default request after an explicit edit resets the numeric value. Finite,
integer/fixed representation, checkbox and mask bounds remain checked. Explicit
out-of-range numbers remain rejected, including numbers equal to the default.
Default mode is not supported for colors, points or arbitrary data.

`encode_default_interactive_payload` / `normalize_default_interactive_parameters`
mark out-of-range numeric discovery defaults instead of clamping them. In-range
values retain the existing explicit encoding. Direct API clients can opt into
default mode regardless of range. Windows UI discovery retains the declared
value, passive controls do not clamp it, and edits clear default mode; Reset
restores it. Callers editing a previously marked descriptor must likewise clear
`use_declared_default` to submit an explicit number.

Behavioral coverage: broker encoding/serde and session transport tests; the
worker `--kind classic --self-test-declared-defaults` route parses real payloads,
checks refusal/atomicity, and reads applied ABI bytes for integer, popup, fixed
and float parameters. The same self-test runs on all worker routes. These are
host contract tests, not an AE pixel-equivalence claim.
