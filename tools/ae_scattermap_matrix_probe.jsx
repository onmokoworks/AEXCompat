(function () {
    function esc(value) {
        return '"' + String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')
            .replace(/\r/g, "\\r").replace(/\n/g, "\\n") + '"';
    }
    function writeText(path, text, append) {
        var file = new File(path);
        if (!file.parent.exists) { file.parent.create(); }
        file.encoding = "UTF-8";
        if (!file.open(append ? "a" : "w")) { throw new Error("cannot create " + path); }
        file.write(text); file.close();
    }
    function marker(text) {
        var path = $.getenv("AEXCOMPAT_AE_MARKER");
        if (path) { writeText(path, String(text) + "\n", true); }
    }
    function setDefaults(effect) {
        effect.property("Scatter Amount").setValue(5);
        effect.property("Direction").setValue(3);
        effect.property("Random Seed").setValue(0);
        effect.property("Mix with Original").setValue(100);
        effect.property("Invert Map").setValue(0);
    }
    function renderCase(comp, effect, outputDir, item) {
        setDefaults(effect);
        if (item.amount !== undefined) { effect.property("Scatter Amount").setValue(item.amount); }
        if (item.direction !== undefined) { effect.property("Direction").setValue(item.direction); }
        if (item.seed !== undefined) { effect.property("Random Seed").setValue(item.seed); }
        if (item.mix !== undefined) { effect.property("Mix with Original").setValue(item.mix); }
        var output = new File(outputDir + "/" + item.id + ".png");
        if (output.exists) { throw new Error("output already exists: " + item.id); }
        marker("render " + item.id);
        comp.saveFrameToPng(0, output);
        return '{"case_id":' + esc(item.id) + ',"render_api_completed":true}';
    }

    var inputPath = $.getenv("AEXCOMPAT_AE_INPUT");
    var outputDir = $.getenv("AEXCOMPAT_AE_OUTPUT_DIR");
    var reportPath = $.getenv("AEXCOMPAT_AE_REPORT");
    var cases = [
        {id:"default"}, {id:"identity", amount:0},
        {id:"horizontal", direction:1}, {id:"vertical", direction:2},
        {id:"amount_max", amount:500}, {id:"seed_max", seed:10000},
        {id:"mix_zero", mix:0}
    ];
    var results = [], errorText = "";
    marker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        if (!inputPath || !outputDir || !reportPath) { throw new Error("required environment missing"); }
        app.newProject(); app.project.bitsPerChannel = 8;
        try { app.project.workingSpace = "None"; } catch (colorError) {}
        var footage = app.project.importFile(new ImportOptions(new File(inputPath)));
        var comp = app.project.items.addComp("AEXCompatMatrixProbe", 16, 12, 1, 1 / 24, 24);
        var effect = comp.layers.add(footage).property("ADBE Effect Parade").addProperty("ScatterMap");
        for (var i = 0; i < cases.length; i += 1) {
            results.push(renderCase(comp, effect, outputDir, cases[i]));
        }
    } catch (error) {
        errorText = String(error && error.message ? error.message : error);
        marker("probe error: " + errorText);
    } finally {
        var json = '{"schema_version":1,"app_version":' + esc(app.version) +
            ',"cases":[' + results.join(",") + '],"error":' + esc(errorText) + '}\n';
        try { writeText(reportPath, json, false); } catch (reportError) { marker("report error: " + reportError); }
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false); app.quit();
    }
}());
