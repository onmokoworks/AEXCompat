// Builds the selector-timeline observation project (issue #98 stage 0 items
// 1/2/6, issue #102): one comp over a still footage item, the probe effect
// applied, "Drive" optionally animated across the comp, "Probe Mode" static,
// and one render-queue item spanning every frame with the default output
// module (the rendered pixels are discarded; the observation is the probe's
// JSONL sidecar). Configuration arrives via environment variables and the
// result manifest is hand-serialized (AE 25.3 ExtendScript has no JSON
// global; see docs/AE_ORACLE_NTSC_RS_CAPTURE_2026-07-18.md).
(function () {
    function requiredEnv(name) {
        var value = $.getenv(name);
        if (!value) {
            throw new Error("missing environment variable: " + name);
        }
        return value;
    }

    function jsonString(value) {
        var escaped = String(value)
            .replace(/\\/g, "\\\\")
            .replace(/"/g, '\\"')
            .replace(/[\x00-\x1f]/g, function (ch) {
                var hex = ch.charCodeAt(0).toString(16);
                while (hex.length < 4) {
                    hex = "0" + hex;
                }
                return "\\u" + hex;
            });
        return '"' + escaped + '"';
    }

    function jsonValue(value) {
        if (typeof value === "number" || typeof value === "boolean") {
            return String(value);
        }
        return jsonString(value);
    }

    function serialize(payload) {
        var parts = [];
        for (var key in payload) {
            if (payload.hasOwnProperty(key)) {
                parts.push(jsonString(key) + ":" + jsonValue(payload[key]));
            }
        }
        return "{" + parts.join(",") + "}";
    }

    function writeJson(path, payload) {
        var file = new File(path);
        file.encoding = "UTF-8";
        if (!file.open("w")) {
            throw new Error("could not create result JSON");
        }
        file.write(serialize(payload));
        file.close();
    }

    var resultPath = requiredEnv("AEXCOMPAT_SELT_RESULT");
    var ownsProject = false;
    var safeToQuit = false;
    try {
        if (app.project && (app.project.numItems !== 0 || app.project.file !== null)) {
            throw new Error("refusing to modify a non-empty or saved After Effects project");
        }
        safeToQuit = true;
        if (!app.project) {
            app.newProject();
        }
        ownsProject = true;

        var inputFile = new File(requiredEnv("AEXCOMPAT_SELT_INPUT"));
        var projectFile = new File(requiredEnv("AEXCOMPAT_SELT_PROJECT"));
        var outputFile = new File(requiredEnv("AEXCOMPAT_SELT_OUTPUT"));
        var effectName = requiredEnv("AEXCOMPAT_SELT_EFFECT");
        var fps = Number(requiredEnv("AEXCOMPAT_SELT_FPS"));
        var durationFrames = Number(requiredEnv("AEXCOMPAT_SELT_DURATION"));
        var animateDrive = requiredEnv("AEXCOMPAT_SELT_DRIVE_ANIMATE") === "1";
        var probeMode = Number(requiredEnv("AEXCOMPAT_SELT_MODE"));
        if (!(fps > 0) || !(durationFrames > 0)) {
            throw new Error("fps and duration must be positive");
        }
        if (!inputFile.exists) {
            throw new Error("input footage does not exist");
        }

        var footage = app.project.importFile(new ImportOptions(inputFile));
        var comp = app.project.items.addComp(
            "SelectorTimeline", footage.width, footage.height, 1.0,
            durationFrames / fps, fps
        );
        var layer = comp.layers.add(footage);
        layer.outPoint = comp.duration;
        var effect = layer.property("ADBE Effect Parade").addProperty(effectName);
        if (!effect) {
            throw new Error("effect could not be added by match or display name");
        }
        var drive = effect.property("Drive");
        var mode = effect.property("Probe Mode");
        if (!drive || !mode) {
            throw new Error("probe parameters were not found on the effect");
        }
        if (animateDrive) {
            drive.setValueAtTime(0, 0);
            drive.setValueAtTime((durationFrames - 1) / fps, 100);
        }
        mode.setValue(probeMode);

        var renderItem = app.project.renderQueue.items.add(comp);
        renderItem.timeSpanStart = 0;
        renderItem.timeSpanDuration = comp.duration;
        var outputModule = renderItem.outputModule(1);
        outputModule.file = outputFile;
        app.project.save(projectFile);

        writeJson(resultPath, {
            schema_version: 1,
            status: "prepared",
            ae_version: app.version,
            project: projectFile.fsName,
            comp: comp.name,
            effect_name: effect.name,
            effect_match_name: effect.matchName,
            width: comp.width,
            height: comp.height,
            fps: fps,
            duration_frames: durationFrames,
            drive_animated: animateDrive,
            probe_mode: probeMode,
            output_module_template: "default"
        });
    } catch (error) {
        try {
            writeJson(resultPath, {schema_version: 1, status: "failed", error: String(error)});
        } catch (_) {
        }
    } finally {
        if (safeToQuit && ownsProject) {
            if (app.project) {
                app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            }
            app.quit();
        }
    }
}());
