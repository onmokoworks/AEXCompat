(function () {
    var resultPath = Folder.temp.fsName + "/aexcompat-pf-batch-sampling-run.result.json";
    function writeResult(payload) {
        var file = new File(resultPath); file.encoding = "UTF-8"; file.open("w");
        file.write(JSON.stringify(payload)); file.close();
    }
    try {
        if (!app.project) app.newProject();
        if (app.project.numItems !== 0 || app.project.file !== null)
            throw new Error("refusing to modify a non-empty or saved project");
        var comp = app.project.items.addComp("AEXCompat Batch Sampling Oracle", 8, 8, 1, 1 / 30, 30);
        var layer = comp.layers.addSolid([1, 0.5, 0], "Oracle Input", 8, 8, 1);
        var effect = layer.property("ADBE Effect Parade").addProperty("AEXCompat PF Batch Sampling V1");
        if (!effect) throw new Error("oracle effect was not found");
        var output = new File(Folder.temp.fsName + "/aexcompat-pf-batch-sampling.png");
        comp.saveFrameToPng(0, output);
        writeResult({schema_version: 1, status: "captured", ae_version: app.version,
            effect_name: effect.name, effect_match_name: effect.matchName});
    } catch (error) {
        writeResult({schema_version: 1, status: "failed", error: String(error)});
    } finally {
        if (app.project && app.project.file === null) app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
    }
}());
