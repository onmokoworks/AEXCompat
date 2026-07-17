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
        if (!app.project) {
            app.newProject();
            createdProject = true;
        }
        if (app.project.numItems !== 0 || app.project.file !== null)
            throw new Error("refusing to modify a non-empty or saved After Effects project");
        safeToQuit = true;
        app.project.bitsPerChannel = Number(env("AEXCOMPAT_AE_BPC"));
        var comp = app.project.items.addComp("AEXCompat Path Curve Oracle", 128, 64, 1, 1, 30);
        var layer = comp.layers.addSolid([0, 0, 0], "Oracle Carrier", 128, 64, 1, 1);
        var mask = layer.property("ADBE Mask Parade").addProperty("ADBE Mask Atom");
        var shape = new Shape();
        shape.vertices = [[16, 32], [112, 32]];
        shape.inTangents = [[0, 0], [-32, -24]];
        shape.outTangents = [[32, 24], [0, 0]];
        shape.closed = false;
        mask.property("ADBE Mask Shape").setValue(shape);
        var parade = layer.property("ADBE Effect Parade");
        var requestedName = env("AEXCOMPAT_AE_EFFECT");
        var canAdd = parade.canAddProperty(requestedName);
        if (!canAdd) throw new Error("oracle effect cannot be added");
        var effect = parade.addProperty(requestedName);
        if (!effect) throw new Error("oracle effect was not found");
        var pathParameter = effect.property(1);
        pathParameter.setValue(1);
        var output = new File(env("AEXCOMPAT_AE_OUTPUT"));
        comp.saveFrameToPng(0, output);
        writeResult({schema_version: 1, status: "captured", ae_version: app.version,
            effect_name: effect.name, effect_match_name: effect.matchName,
            can_add_property: canAdd, path_value: pathParameter.value,
            width: comp.width, height: comp.height, bpc: app.project.bitsPerChannel,
            output: output.fsName});
    } catch (error) {
        try { writeResult({schema_version: 1, status: "failed", error: String(error)}); } catch (_) {}
    } finally {
        if (safeToQuit) {
            if (app.project && (createdProject || app.project.file === null))
                app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            app.quit();
        }
    }
}());
