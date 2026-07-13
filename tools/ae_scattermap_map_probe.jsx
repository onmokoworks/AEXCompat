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
    function importRequired(path) {
        var file = new File(path);
        if (!file.exists) { throw new Error("input missing: " + path); }
        return app.project.importFile(new ImportOptions(file));
    }
    function render(comp, outputDir, caseId) {
        var file = new File(outputDir + "/" + caseId + ".png");
        if (file.exists) { throw new Error("output exists: " + caseId); }
        marker("render " + caseId);
        comp.saveFrameToPng(0, file);
        return '{"case_id":' + esc(caseId) + ',"render_api_completed":true}';
    }

    var sourcePath = $.getenv("AEXCOMPAT_AE_INPUT");
    var mapSmallPath = $.getenv("AEXCOMPAT_AE_MAP_SMALL");
    var mapFullPath = $.getenv("AEXCOMPAT_AE_MAP_FULL");
    var outputDir = $.getenv("AEXCOMPAT_AE_OUTPUT_DIR");
    var reportPath = $.getenv("AEXCOMPAT_AE_REPORT");
    var results = [], errorText = "";
    marker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        app.newProject(); app.project.bitsPerChannel = 8;
        try { app.project.workingSpace = "None"; } catch (colorError) {}
        var source = importRequired(sourcePath);
        var mapSmall = importRequired(mapSmallPath);
        var mapFull = importRequired(mapFullPath);
        var comp = app.project.items.addComp("AEXCompatMapProbe", 11, 7, 1, 1 / 24, 24);
        var sourceLayer = comp.layers.add(source);
        var effect = sourceLayer.property("ADBE Effect Parade").addProperty("ScatterMap");
        var smallLayer = comp.layers.add(mapSmall); smallLayer.enabled = false;
        var fullLayer = comp.layers.add(mapFull); fullLayer.enabled = false;
        var mapProperty = effect.property("Scatter Map");
        var invertProperty = effect.property("Invert Map");

        mapProperty.setValue(smallLayer.index); invertProperty.setValue(0);
        results.push(render(comp, outputDir, "connected_map"));
        mapProperty.setValue(fullLayer.index); invertProperty.setValue(1);
        results.push(render(comp, outputDir, "inverted_map"));
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
