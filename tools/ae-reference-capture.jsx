(function () {
    function env(name) {
        var value = $.getenv(name);
        if (!value) {
            throw new Error("missing environment variable: " + name);
        }
        return value;
    }

    // After Effects 25.3 ExtendScript has no JSON object, and referencing the
    // missing global aborts the script mid-write, so serialize by hand.
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

    function writeResult(payload) {
        var file = new File(env("AEXCOMPAT_AE_RESULT"));
        file.encoding = "UTF-8";
        if (!file.open("w")) {
            throw new Error("could not create result file");
        }
        file.write(serialize(payload));
        file.close();
    }

    var createdProject = false;
    var safeToQuit = false;
    try {
        if (app.project && (app.project.numItems !== 0 || app.project.file !== null)) {
            throw new Error("refusing to modify a non-empty or saved After Effects project");
        }
        safeToQuit = true;
        if (!app.project) {
            app.newProject();
            createdProject = true;
        }

        var inputFile = new File(env("AEXCOMPAT_AE_INPUT"));
        var outputFile = new File(env("AEXCOMPAT_AE_OUTPUT"));
        if (!inputFile.exists) {
            throw new Error("input image does not exist");
        }
        var importOptions = new ImportOptions(inputFile);
        var footage = app.project.importFile(importOptions);
        var fps = Number(env("AEXCOMPAT_AE_FPS"));
        var frame = Number(env("AEXCOMPAT_AE_FRAME"));
        var durationFrames = Number(env("AEXCOMPAT_AE_DURATION"));
        var depth = Number(env("AEXCOMPAT_AE_BPC"));
        if (!(fps > 0) || !(frame >= 0) || !(durationFrames > frame) ||
            (depth !== 8 && depth !== 16 && depth !== 32)) {
            throw new Error("invalid render timing or pixel depth");
        }

        app.project.bitsPerChannel = depth;

        // Pin the project color pipeline when requested. AE 25.3 exposes
        // workingSpace / linearizeWorkingSpace as silently-coercing setters
        // (an unknown profile name leaves the previous value), so every set
        // is verified by readback and fails closed on mismatch. The observed
        // state is always recorded in the result for evidence identity.
        var requestedSpace = $.getenv("AEXCOMPAT_AE_WORKING_SPACE");
        var colorPinned = false;
        if (requestedSpace) {
            // Setting the empty string turns color management off; AE reads
            // that state back as "None", so accept "None" as the request.
            app.project.workingSpace =
                requestedSpace === "None" ? "" : requestedSpace;
            if (app.project.workingSpace !== requestedSpace) {
                throw new Error(
                    "working space did not apply: requested " +
                    requestedSpace + ", got " + app.project.workingSpace
                );
            }
            colorPinned = true;
        }
        var requestedLinearize = $.getenv("AEXCOMPAT_AE_LINEARIZE");
        if (requestedLinearize === "0" || requestedLinearize === "1") {
            var wantLinearize = requestedLinearize === "1";
            app.project.linearizeWorkingSpace = wantLinearize;
            if (app.project.linearizeWorkingSpace !== wantLinearize) {
                throw new Error(
                    "linearize working space did not apply: requested " +
                    wantLinearize
                );
            }
            colorPinned = true;
        }

        var comp = app.project.items.addComp(
            "AEXCompat AE Reference",
            footage.width,
            footage.height,
            1.0,
            durationFrames / fps,
            fps
        );
        var layer = comp.layers.add(footage);
        var noEffect = $.getenv("AEXCOMPAT_AE_NO_EFFECT") === "1";
        var effect = null;
        if (!noEffect) {
            effect = layer.property("ADBE Effect Parade").addProperty(env("AEXCOMPAT_AE_EFFECT"));
            if (!effect) {
                throw new Error("effect could not be added by match or display name");
            }
        }
        // Optional single-parameter override for oracle captures that must
        // match a host render with a non-default value.
        var paramName = $.getenv("AEXCOMPAT_AE_PARAM_NAME");
        var paramApplied = null;
        if (paramName) {
            if (!effect) {
                throw new Error("AEXCOMPAT_AE_PARAM_NAME requires an effect; it cannot be combined with AEXCOMPAT_AE_NO_EFFECT");
            }
            var paramValueRaw = $.getenv("AEXCOMPAT_AE_PARAM_VALUE");
            var paramValue = Number(paramValueRaw);
            if (paramValueRaw === null || paramValueRaw === "" || isNaN(paramValue)) {
                throw new Error("AEXCOMPAT_AE_PARAM_VALUE must be a number when AEXCOMPAT_AE_PARAM_NAME is set");
            }
            var paramProp = effect.property(paramName);
            if (!paramProp) {
                throw new Error("effect parameter not found: " + paramName);
            }
            paramProp.setValue(paramValue);
            paramApplied = { name: paramName, value: paramProp.value };
        }
        comp.time = frame / fps;
        if (typeof comp.saveFrameToPng !== "function") {
            throw new Error("CompItem.saveFrameToPng is unavailable");
        }
        // saveFrameToPng queues an asynchronous write; quitting immediately
        // discards it, so poll until the file exists. The bound comes from the
        // runner so the JSX never outlives the outer watchdog.
        var saveTimeoutMs = Number($.getenv("AEXCOMPAT_AE_SAVE_TIMEOUT_MS"));
        if (!(saveTimeoutMs > 0)) {
            saveTimeoutMs = 120000;
        }
        comp.saveFrameToPng(comp.time, outputFile);
        var waitedMs = 0;
        while (!outputFile.exists && waitedMs < saveTimeoutMs) {
            $.sleep(500);
            waitedMs += 500;
        }
        if (!outputFile.exists) {
            throw new Error("saveFrameToPng did not produce the PNG within " + saveTimeoutMs + "ms");
        }
        var payload = {
            schema_version: 1,
            status: "captured",
            ae_version: app.version,
            effect_applied: !noEffect,
            effect_name: effect ? effect.name : "",
            effect_match_name: effect ? effect.matchName : "",
            width: comp.width,
            height: comp.height,
            frame: frame,
            fps: fps,
            duration_frames: durationFrames,
            bpc: depth,
            color_pinned: colorPinned,
            working_space: app.project.workingSpace,
            working_gamma: app.project.workingGamma,
            linearize_working_space: app.project.linearizeWorkingSpace,
            linear_blending: app.project.linearBlending,
            compensate_for_scene_referred_profiles:
                app.project.compensateForSceneReferredProfiles,
            save_wait_ms: waitedMs,
            output: outputFile.fsName
        };
        if (paramApplied) {
            payload.param_name = paramApplied.name;
            payload.param_value = paramApplied.value;
        }
        // The effect's own parameters as AE materializes them for this
        // capture (index|name|matchName|propertyValueType|value, ";;"
        // separated), so a host default can be compared against AE's
        // (issue #1253: Reshape declares its popups with dephault 0 and AE
        // reads them back as 1). Values AE cannot read (NO_VALUE groups,
        // CUSTOM_VALUE arbitrary data) are recorded as the error text.
        if (effect) {
            var dumped = [];
            for (var pi = 1; pi <= effect.numProperties; pi++) {
                var prop = effect.property(pi);
                var propValue = "";
                try {
                    propValue = String(prop.value);
                } catch (readError) {
                    propValue = "<" + String(readError) + ">";
                }
                dumped.push(String(pi) + "|" + prop.name + "|" + prop.matchName +
                    "|" + String(prop.propertyValueType) + "|" + propValue);
            }
            payload.params = dumped.join(";;");
        }
        writeResult(payload);
    } catch (error) {
        try {
            writeResult({
                schema_version: 1,
                status: "failed",
                error: String(error)
            });
        } catch (_) {
        }
    } finally {
        if (safeToQuit) {
            if (app.project && (createdProject || app.project.file === null)) {
                app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            }
            app.quit();
        }
    }
}());
