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

    function findOpenExrTemplate(outputModule) {
        var templates = outputModule.templates;
        for (var i = 0; i < templates.length; i += 1) {
            if (/openexr|open exr|\bexr\b/i.test(templates[i])) {
                return templates[i];
            }
        }
        return null;
    }

    var resultPath = requiredEnv("AEXCOMPAT_AE_ORACLE_RESULT");
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

        var inputFile = new File(requiredEnv("AEXCOMPAT_AE_ORACLE_INPUT"));
        var projectFile = new File(requiredEnv("AEXCOMPAT_AE_ORACLE_PROJECT"));
        var outputFile = new File(requiredEnv("AEXCOMPAT_AE_ORACLE_OUTPUT"));
        var effectName = requiredEnv("AEXCOMPAT_AE_ORACLE_EFFECT");
        var depth = Number(requiredEnv("AEXCOMPAT_AE_ORACLE_BPC"));
        var fps = Number(requiredEnv("AEXCOMPAT_AE_ORACLE_FPS"));
        var durationFrames = Number(requiredEnv("AEXCOMPAT_AE_ORACLE_DURATION"));
        if (!inputFile.exists) {
            throw new Error("input image does not exist");
        }
        if ((depth !== 8 && depth !== 16 && depth !== 32) || !(fps > 0) || !(durationFrames > 0)) {
            throw new Error("invalid pixel depth or timing");
        }

        app.project.bitsPerChannel = depth;
        var footage = app.project.importFile(new ImportOptions(inputFile));
        var compName = "AEXCompat Oracle " + depth + "bpc";
        var comp = app.project.items.addComp(
            compName, footage.width, footage.height, 1.0, durationFrames / fps, fps
        );
        var layer = comp.layers.add(footage);
        var effect = layer.property("ADBE Effect Parade").addProperty(effectName);
        if (!effect) {
            throw new Error("effect could not be added by match or display name");
        }

        var renderItem = app.project.renderQueue.items.add(comp);
        renderItem.timeSpanStart = 0;
        renderItem.timeSpanDuration = 1 / fps;
        var outputModule = renderItem.outputModule(1);
        var exrTemplate = findOpenExrTemplate(outputModule);
        if (!exrTemplate) {
            throw new Error("no OpenEXR output module template is installed; available=" +
                outputModule.templates.join(" | "));
        }
        outputModule.applyTemplate(exrTemplate);

        var fullFloatApplied = /32.*float/i.test(exrTemplate);
        try {
            outputModule.setSettings({
                "Output Module Settings": {
                    "Channels": "RGB + Alpha",
                    "Depth": "Floating Point",
                    "Color": "Straight (Unmatted)"
                }
            });
            fullFloatApplied = true;
        } catch (_) {
            // Template settings are localized/version-specific; the manifest makes fallback explicit.
        }
        outputModule.file = outputFile;
        app.project.save(projectFile);

        writeJson(resultPath, {
            schema_version: 1,
            status: "prepared",
            ae_version: app.version,
            project: projectFile.fsName,
            output: outputFile.fsName,
            comp: compName,
            effect_name: effect.name,
            effect_match_name: effect.matchName,
            bpc: depth,
            width: comp.width,
            height: comp.height,
            fps: fps,
            duration_frames: durationFrames,
            output_module_template: exrTemplate,
            full_float_settings_applied: fullFloatApplied
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
