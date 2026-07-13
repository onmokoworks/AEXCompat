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
    var outputDir = $.getenv("AEXCOMPAT_AE_OUTPUT_DIR");
    var reportPath = $.getenv("AEXCOMPAT_AE_REPORT");
    var results = [], errorText = "";
    marker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        app.newProject(); app.project.bitsPerChannel = 8;
        try { app.project.workingSpace = "None"; } catch (colorError) {}
        var footage = app.project.importFile(new ImportOptions(new File(inputPath)));
        var comp = app.project.items.addComp("AEXCompatAlphaProbe", 16, 12, 1, 1 / 24, 24);
        var layer = comp.layers.add(footage);
        var effect = layer.property("ADBE Effect Parade").addProperty("ScatterMap");
        var amount = effect.property("Scatter Amount");
        var cases = [{id:"identity", value:0}, {id:"default", value:5}];
        for (var i = 0; i < cases.length; i += 1) {
            amount.setValue(cases[i].value);
            var output = new File(outputDir + "/" + cases[i].id + ".png");
            if (output.exists) { throw new Error("output already exists: " + cases[i].id); }
            marker("render " + cases[i].id);
            comp.saveFrameToPng(0, output);
            results.push('{"case_id":' + esc(cases[i].id) +
                ',"amount":' + cases[i].value + ',"render_api_completed":true}');
        }
    } catch (error) {
        errorText = String(error && error.message ? error.message : error);
        marker("probe error: " + errorText);
    } finally {
        var json = '{"schema_version":1,"app_version":' + esc(app.version) +
            ',"input_kind":"variable_alpha_rgba8","cases":[' + results.join(",") +
            '],"error":' + esc(errorText) + '}\n';
        try { write(reportPath, json, false); } catch (reportError) { marker("report error: " + reportError); }
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false); app.quit();
    }
}());
