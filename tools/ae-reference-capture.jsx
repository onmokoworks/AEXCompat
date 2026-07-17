(function () {
    function env(name) {
        var value = $.getenv(name);
        if (!value) {
            throw new Error("missing environment variable: " + name);
        }
        return value;
    }

    function writeResult(payload) {
        var file = new File(env("AEXCOMPAT_AE_RESULT"));
        file.encoding = "UTF-8";
        if (!file.open("w")) {
            throw new Error("could not create result file");
        }
        file.write(JSON.stringify(payload));
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
        var comp = app.project.items.addComp(
            "AEXCompat AE Reference",
            footage.width,
            footage.height,
            1.0,
            durationFrames / fps,
            fps
        );
        var layer = comp.layers.add(footage);
        var effect = layer.property("ADBE Effect Parade").addProperty(env("AEXCOMPAT_AE_EFFECT"));
        if (!effect) {
            throw new Error("effect could not be added by match or display name");
        }
        comp.time = frame / fps;
        if (typeof comp.saveFrameToPng !== "function") {
            throw new Error("CompItem.saveFrameToPng is unavailable");
        }
        comp.saveFrameToPng(comp.time, outputFile);
        writeResult({
            schema_version: 1,
            status: "captured",
            ae_version: app.version,
            effect_name: effect.name,
            effect_match_name: effect.matchName,
            width: comp.width,
            height: comp.height,
            frame: frame,
            fps: fps,
            duration_frames: durationFrames,
            bpc: depth,
            output: outputFile.fsName
        });
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
