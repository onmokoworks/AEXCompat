# Numeric declared defaults

`InteractiveParameter.use_declared_default` (serde default `false`) selects the
numeric default declared by the **loaded plug-in**, clamped to its declared valid
range for integer/fixed/float sliders and popups. Native image and interactive
frame APIs share this transport. The caller's `value` is ignored in this mode;
it is not a range-check exemption for a supplied number.

The worker payload is `v7|param_5@5:f64=default` (or `i32=default` for an integer,
popup, checkbox or path). v7 also accepts the older typed assignments. The
worker validates the actual slot/type and resolves the effective default on every apply,
so a default request after an explicit edit resets the numeric value. Finite,
integer/fixed representation, checkbox and mask bounds remain checked. Explicit
out-of-range numbers remain rejected, including numbers equal to the default.
Default mode is not supported for colors, points or arbitrary data.
The strict declarative fixture v1/v2 schema is unchanged: explicit mode omits
the new field on serialization; default mode is for native/interactive APIs.

`encode_default_interactive_payload` / `normalize_default_interactive_parameters`
mark out-of-range numeric discovery defaults instead of clamping them. In-range
values retain the existing explicit encoding. Direct API clients can opt into
default mode regardless of range. Windows UI discovery retains the declared
value, passive controls do not clamp it, and edits clear default mode; Reset
restores it. Callers editing a previously marked descriptor must likewise clear
`use_declared_default` to submit an explicit number.

Discovery descriptors retain the declaration as provenance; they are not the
effective render value. The worker normalizes at `initialize_parameter_definitions`
(before first use) and at default assignment/reset, leaving the stored declaration
unchanged. The worker's `current_value` tracks the effective value. A caller or
bridge echo of the input descriptor is still the declaration, not a readback
of that worker state.
UI slider ranges do not replace declared valid ranges. Nonfinite or unrepresentable
default assignments and invalid default ranges are rejected, not rescued by clamp.

This corrects #1801 based on the T28 AE oracle (#1803): KiraKira declares Gain
0.1 with valid range [1,100], but AE 25.1 reads back 1 immediately after adding
the effect, before and after rendering, in all 12 captures (two inputs, three
depths, two repeats). The captures do not identify which internal AE callback
performs normalization. This host boundary reproduces the observed first-use
value; it is not a claim about AE's internal implementation. Upper-bound and
other numeric-type coverage are general host contract tests, not additional AE
oracle observations. Checkbox/path validation remains separate and unchanged.

Behavioral coverage: broker encoding/serde and session transport tests; the
worker `--kind classic --self-test-declared-defaults` route parses real payloads,
checks refusal/atomicity, and reads applied ABI bytes for integer, popup, fixed
and float parameters. The same self-test runs on all worker routes. These are
host contract tests, not an AE pixel-equivalence claim.
