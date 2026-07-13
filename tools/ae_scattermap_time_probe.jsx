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
        var comp = app.project.items.addComp("AEXCompatTimeProbe", 16, 12, 1, 3 / 24, 24);
        var layer = comp.layers.add(footage);
        layer.outPoint = comp.duration;
        layer.property("ADBE Effect Parade").addProperty("ScatterMap");
        var times = [0, 1 / 24];
        for (var i = 0; i < times.length; i += 1) {
            var output = new File(outputDir + "/frame" + i + ".png");
            if (output.exists) { throw new Error("output already exists for frame " + i); }
            marker("render frame " + i);
            comp.saveFrameToPng(times[i], output);
            results.push('{"frame":' + i + ',"time_seconds":' + times[i] +
                ',"render_api_completed":true}');
        }
    } catch (error) {
        errorText = String(error && error.message ? error.message : error);
        marker("probe error: " + errorText);
    } finally {
        var json = '{"schema_version":1,"app_version":' + esc(app.version) +
            ',"frame_rate":24,"cases":[' + results.join(",") +
            '],"error":' + esc(errorText) + '}\n';
        try { write(reportPath, json, false); } catch (reportError) { marker("report error: " + reportError); }
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false); app.quit();
    }
}());
