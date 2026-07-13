(function () {
    function esc(value) {
        return '"' + String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')
            .replace(/\r/g, "\\r").replace(/\n/g, "\\n") + '"';
    }

    function writeText(path, text, append) {
        if (!path) { return; }
        var file = new File(path);
        if (!file.parent.exists) { file.parent.create(); }
        file.encoding = "UTF-8";
        if (!file.open(append ? "a" : "w")) { throw new Error("cannot create " + path); }
        file.write(text);
        file.close();
    }

    function marker(text) {
        writeText($.getenv("AEXCOMPAT_AE_MARKER"), String(text) + "\n", true);
    }

    function propertyJson(property) {
        var value = "";
        try { value = String(property.value); } catch (error) { value = "<unavailable>"; }
        return '{"index":' + property.propertyIndex +
            ',"name":' + esc(property.name) +
            ',"match_name":' + esc(property.matchName) +
            ',"value":' + esc(value) + '}';
    }

    var reportPath = $.getenv("AEXCOMPAT_AE_REPORT");
    var inputPath = $.getenv("AEXCOMPAT_AE_INPUT");
    var outputPath = $.getenv("AEXCOMPAT_AE_OUTPUT");
    var report = { appVersion: app.version, found: false, added: false,
        rendered: false, properties: [], error: "" };

    marker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        if (!inputPath || !outputPath || !reportPath) {
            throw new Error("AEXCOMPAT_AE_INPUT, OUTPUT, and REPORT are required");
        }
        app.newProject();
        app.project.bitsPerChannel = 8;
        try { app.project.workingSpace = "None"; } catch (colorError) {}

        var inputFile = new File(inputPath);
        if (!inputFile.exists) { throw new Error("input does not exist"); }
        marker("import input");
        var footage = app.project.importFile(new ImportOptions(inputFile));
        var comp = app.project.items.addComp("AEXCompatRenderProbe", 16, 12, 1, 1 / 24, 24);
        var layer = comp.layers.add(footage);
        var parade = layer.property("ADBE Effect Parade");
        report.found = parade.canAddProperty("ScatterMap");
        marker("add effect found=" + report.found);
        var effect = parade.addProperty("ScatterMap");
        report.added = effect !== null;
        for (var i = 1; effect && i <= effect.numProperties; i += 1) {
            report.properties.push(propertyJson(effect.property(i)));
        }

        var outputFile = new File(outputPath);
        if (outputFile.exists) { throw new Error("output already exists"); }
        marker("render frame");
        comp.saveFrameToPng(0, outputFile);
        // AE keeps File length stale until after the script exits. The external
        // verifier owns existence, decode, and pixel-parity validation.
        report.rendered = true;
        marker("render API complete");
    } catch (error) {
        report.error = String(error && error.message ? error.message : error);
        marker("probe error: " + report.error);
    } finally {
        var json = '{"schema_version":1,"app_version":' + esc(report.appVersion) +
            ',"found":' + report.found + ',"added":' + report.added +
            ',"rendered":' + report.rendered + ',"properties":[' + report.properties.join(",") +
            '],"error":' + esc(report.error) + '}\n';
        try { writeText(reportPath, json, false); } catch (reportError) { marker("report error: " + reportError); }
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false);
        app.quit();
    }
}());
