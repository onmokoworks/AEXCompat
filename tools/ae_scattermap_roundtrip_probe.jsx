(function () {
    function esc(value) {
        return '"' + String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')
            .replace(/\r/g, "\\r").replace(/\n/g, "\\n") + '"';
    }
    function write(path, text, append) {
        var file = new File(path);
        if (!file.parent.exists) { file.parent.create(); }
        file.encoding = "UTF-8";
        if (!file.open(append ? "a" : "w")) { throw new Error("cannot write " + path); }
        file.write(text); file.close();
    }
    function marker(text) {
        var path = $.getenv("AEXCOMPAT_AE_MARKER");
        if (path) { write(path, String(text) + "\n", true); }
    }
    var inputPath = $.getenv("AEXCOMPAT_AE_INPUT");
    var projectPath = $.getenv("AEXCOMPAT_AE_PROJECT");
    var outputPath = $.getenv("AEXCOMPAT_AE_OUTPUT");
    var reportPath = $.getenv("AEXCOMPAT_AE_REPORT");
    var report = {saved:false, reopened:false, effectFound:false, seed:-1,
        renderApiCompleted:false, error:""};
    marker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        app.newProject(); app.project.bitsPerChannel = 8;
        try { app.project.workingSpace = "None"; } catch (colorError) {}
        var footage = app.project.importFile(new ImportOptions(new File(inputPath)));
        var comp = app.project.items.addComp("AEXCompatRoundtrip", 16, 12, 1, 1 / 24, 24);
        var layer = comp.layers.add(footage); layer.name = "ScatterMapSource";
        var effect = layer.property("ADBE Effect Parade").addProperty("ScatterMap");
        effect.property("Random Seed").setValue(10000);
        var projectFile = new File(projectPath);
        if (projectFile.exists) { throw new Error("project output already exists"); }
        marker("save project");
        report.saved = app.project.save(projectFile);
        app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);

        marker("reopen project");
        app.open(projectFile);
        report.reopened = app.project !== null;
        var reopenedComp = null;
        for (var i = 1; i <= app.project.numItems; i += 1) {
            if (app.project.item(i).name === "AEXCompatRoundtrip") { reopenedComp = app.project.item(i); break; }
        }
        if (!reopenedComp) { throw new Error("reopened composition missing"); }
        var reopenedLayer = reopenedComp.layer("ScatterMapSource");
        var reopenedEffect = reopenedLayer.property("ADBE Effect Parade").property("ScatterMap");
        report.effectFound = reopenedEffect !== null;
        report.seed = reopenedEffect.property("Random Seed").value;
        if (report.seed !== 10000) { throw new Error("seed did not persist"); }
        var outputFile = new File(outputPath);
        if (outputFile.exists) { throw new Error("render output already exists"); }
        marker("render reopened project");
        reopenedComp.saveFrameToPng(0, outputFile);
        report.renderApiCompleted = true;
    } catch (error) {
        report.error = String(error && error.message ? error.message : error);
        marker("probe error: " + report.error);
    } finally {
        var json = '{"schema_version":1,"app_version":' + esc(app.version) +
            ',"saved":' + report.saved + ',"reopened":' + report.reopened +
            ',"effect_found":' + report.effectFound + ',"seed":' + report.seed +
            ',"render_api_completed":' + report.renderApiCompleted +
            ',"error":' + esc(report.error) + '}\n';
        try { write(reportPath, json, false); } catch (reportError) { marker("report error: " + reportError); }
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false); app.quit();
    }
}());
