(function () {
    var safeToQuit = false;
    var createdProject = false;
    function env(name) {
        var value = $.getenv(name);
        if (!value) throw new Error("missing environment variable " + name);
        return value;
    }
    function writeResult(payload) {
        var file = new File(env("AEXCOMPAT_AE_RESULT"));
        file.encoding = "UTF-8";
        if (!file.open("w")) throw new Error("could not open result file");
        file.write(JSON.stringify(payload));
        file.close();
    }
    try {
        if (!app.project) { app.newProject(); createdProject = true; }
        if (app.project.numItems !== 0 || app.project.file !== null)
            throw new Error("refusing to modify a non-empty or saved After Effects project");
        safeToQuit = true;
        app.project.bitsPerChannel = Number(env("AEXCOMPAT_AE_BPC"));
        var input = new File(env("AEXCOMPAT_AE_INPUT"));
        if (!input.exists) throw new Error("oracle input image does not exist");
        var footage = app.project.importFile(new ImportOptions(input));
        var comp = app.project.items.addComp("AEXCompat Multi Matrix Oracle",
            footage.width, footage.height, 1.0, 1.0 / 30.0, 30);
        var layer = comp.layers.add(footage);
        layer.name = "Oracle Input";
        var parade = layer.property("ADBE Effect Parade");
        var requestedName = env("AEXCOMPAT_AE_EFFECT");
        var canAdd = parade.canAddProperty(requestedName);
        if (!canAdd) throw new Error("oracle effect cannot be added");
        var effect = parade.addProperty(requestedName);
        if (!effect) throw new Error("oracle effect was not found");
        var output = new File(env("AEXCOMPAT_AE_OUTPUT"));
        comp.saveFrameToPng(1.0 / 30.0, output);
        writeResult({schema_version: 1, status: "captured", ae_version: app.version,
            effect_name: effect.name, effect_match_name: effect.matchName,
            can_add_property: canAdd, bpc: app.project.bitsPerChannel, output: output.fsName});
    } catch (error) {
        var candidates = [];
        for (var i = 0; app.effects && i < app.effects.length; i += 1) {
            var matchName = String(app.effects[i].matchName);
            var displayName = String(app.effects[i].displayName);
            if (/transform|matrix|aexcompat|pf /i.test(matchName + " " + displayName))
                candidates.push({match_name: matchName, display_name: displayName});
        }
        try { writeResult({schema_version: 1, status: "failed", error: String(error),
            effect_candidates: candidates}); } catch (_) {}
    } finally {
        if (safeToQuit) {
            if (app.project && (createdProject || app.project.file === null))
                app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            app.quit();
        }
    }
}());
