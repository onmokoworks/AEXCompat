# ScatterMap Broker Parameter Gate Result

The broker now owns the five production-observed ScatterMap numeric
descriptors. Caller JSON supplies values only and cannot replace valid ranges,
types, the plug-in id, or the schema version. Unknown and duplicate fields are
rejected by strict deserialization before a decision report is created.

The native broker CLI was built and executed against two fixed local requests:

| Case | Exit | Decision | Native process |
| --- | ---: | --- | --- |
| all observed valid endpoints | 0 | dispatch permitted | not started |
| Direction 4 (valid 1 to 3) | 3 | rejected | not started |
| unknown assignment field | 2 | structurally rejected, no report | not started |

Both reports were create-new under `target/render-request-results`. The rejected
report records `parameter_out_of_range`, the exact 1-to-3 range,
`native_dispatch_permitted: false`, and `native_process_started: false`.
The structurally invalid request emitted only `render request validation failed`;
it exposed no path or parser detail and created no output. Both request and
output paths are `.json`-only, traversal-denied, and canonical-root checked.

This closes the previous gap between the Python conformance gate and the
broker-owned pre-dispatch boundary. It intentionally does not claim generalized
rendering yet: an accepted request proves eligibility only. A later stage must
bind the accepted values to a fixed-hash worker invocation and output oracle
without introducing caller-controlled plug-in paths or descriptors.
