(function () {
    var safeToQuit = false;
    var createdProject = false;
    var nativePath = Folder.temp.fsName + "/aexcompat-pf-color-oracle.json";
    function env(name) {
        var value = $.getenv(name);
        if (!value) throw new Error("missing environment variable " + name);
        return value;
    }
    function writeResult(payload) {
        var file = new File(env("AEXCOMPAT_AE_RESULT"));
        file.encoding = "UTF-8";
        if (!file.open("w")) throw new Error("could not open result file");
        file.write(JSON.stringify(payload)); file.close();
    }
    try {
        if (!app.project) { app.newProject(); createdProject = true; }
        if (app.project.numItems !== 0 || app.project.file !== null)
            throw new Error("refusing to modify a non-empty or saved After Effects project");
        safeToQuit = true;
        var native = new File(nativePath); if (native.exists) native.remove();
        var comp = app.project.items.addComp("PF Color Oracle", 2, 2, 1, 1 / 30, 30);
        var solid = comp.layers.addSolid([0.25, 0.5, 0.75], "Oracle Input", 2, 2, 1);
        var parade = solid.property("ADBE Effect Parade");
        var requestedName = env("AEXCOMPAT_AE_EFFECT");
        var canAdd = parade.canAddProperty(requestedName);
        if (!canAdd) throw new Error("oracle effect cannot be added: " + requestedName);
        var effect = parade.addProperty(requestedName);
        if (!effect) throw new Error("oracle effect was not found");
        var depths = [8, 16, 32];
        var depthResults = [];
        var requestedDepth = Number(env("AEXCOMPAT_AE_BPC"));
        var requestedOutput = new File(env("AEXCOMPAT_AE_OUTPUT"));
        var temporaryOutputs = [];
        for (var i = 0; i < depths.length; i += 1) {
            if (native.exists) native.remove();
            app.project.bitsPerChannel = depths[i];
            var depthOutput = depths[i] === requestedDepth ? requestedOutput :
                new File(Folder.temp.fsName + "/aexcompat-pf-color-oracle-" + depths[i] + ".png");
            if (depthOutput.exists) depthOutput.remove();
            comp.saveFrameToPng(0, depthOutput);
            if (depths[i] !== requestedDepth) temporaryOutputs.push(depthOutput);
            if (!native.exists) throw new Error("native result missing at " + depths[i] + " bpc");
            native.open("r"); native.encoding = "UTF-8";
            depthResults.push({depth: depths[i], result: JSON.parse(native.read())}); native.close();
        }
        var parsed = {schema_version: 2, status: "captured", ae_version: app.version,
            requested_effect_name: requestedName, can_add_property: canAdd,
            effect_name: effect.name, effect_match_name: effect.matchName,
            rendered_depths: depths, requested_depth: requestedDepth,
            depth_results: depthResults, output: requestedOutput.fsName};
        writeResult(parsed); native.remove();
        for (var j = 0; j < temporaryOutputs.length; j += 1)
            if (temporaryOutputs[j].exists) temporaryOutputs[j].remove();
    } catch (error) {
        writeResult({schema_version: 1, status: "oracle_not_captured", error: String(error)});
    } finally {
        if (safeToQuit) {
            if (app.project && (createdProject || app.project.file === null))
                app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
            app.quit();
        }
    }
}());
