# Named AEX popup choices in Blender on macOS (#1691)

The Blender AEXCompat descriptor can show integer popup choices by name. In **Render AEX (macOS)**, refresh the plug-in parameters, select a named parameter, click **Add/update selected parameter**, then choose one of its labeled options before baking. The chosen label sets the corresponding integer value in the existing render request. The packed result is connected through Blender's native Image and Composite nodes.

Labels appear only when the AEX description has 2–16 distinct, nonempty names and the declared integer range contains exactly one value for each name. For example, OLM `DistanceGradation.aex` reports `In/Out` as Inside=1, Outside=2, Both=3. Its `Blur Mode` reports three names with a range of 1–5, so that parameter retains numeric editing. OLM `OLMKiraKira.aex` reports `Merge mode` as premultiply=1, add=2; its `Channel` range and labels do not align and remain numeric. Refreshing or changing the plug-in clears the selected overrides. A saved `.blend` retains the label mapping and selected integer.

The macOS smoke runs with legally held, license-free OLM AEX files, the Release harness and guest worker, and Blender 4.5.8 LTS:

```sh
AEXCOMPAT_PLUGIN_ROOT=/path/to/OLM \
AEXCOMPAT_HARNESS="$PWD/broker/target/release/aexcompat-harness" \
AEXCOMPAT_GUEST_WORKER="$PWD/guest/target/release/aex-guest-worker" \
/Applications/Blender.app/Contents/MacOS/Blender --background --factory-startup -t 2 \
  --python tools/blender_aexcompat_popup_smoke.py -- --output-dir target/issue1691/blender
```

Run it again with `--reload` to verify the saved choices and packed output. The smoke checks that DistanceGradation's named Outside choice produces the same pixels as direct integer value 2, and that OLMKiraKira's named add choice changes pixels versus premultiply. It rejects an undeclared choice and a forged response with duplicate numeric choice values. The smoke writes local JSON and a `.blend` file under the ignored output directory; no AEX is committed. After Effects is not launched, and these results do not establish AE pixel parity.
