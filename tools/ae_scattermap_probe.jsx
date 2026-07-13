(function () {
    function esc(value) {
        return '"' + String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')
            .replace(/\r/g, "\\r").replace(/\n/g, "\\n") + '"';
    }
    function writeReport(report) {
        var path = $.getenv("AEXCOMPAT_AE_REPORT");
        if (!path) { throw new Error("AEXCOMPAT_AE_REPORT is required"); }
        var file = new File(path);
        if (!file.parent.exists) { file.parent.create(); }
        file.encoding = "UTF-8";
        if (!file.open("w")) { throw new Error("cannot create report"); }
        file.write('{"schema_version":1,"app_version":' + esc(report.appVersion) +
            ',"found":' + report.found + ',"added":' + report.added +
            ',"match_name":' + esc(report.matchName) + ',"display_name":' + esc(report.displayName) +
            ',"property_count":' + report.propertyCount + ',"error":' + esc(report.error) + '}\n');
        file.close();
    }

    function writeMarker(text) {
        var path = $.getenv("AEXCOMPAT_AE_MARKER");
        if (!path) { return; }
        var file = new File(path);
        if (!file.parent.exists) { file.parent.create(); }
        file.encoding = "UTF-8";
        if (file.open("a")) {
            file.write(String(text) + "\n");
            file.close();
        }
    }

    var report = { appVersion: app.version, found: false, added: false,
        matchName: "", displayName: "", propertyCount: 0, error: "" };
    writeMarker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        writeMarker("new project");
        app.newProject();
        writeMarker("scan effects");
        for (var i = 0; app.effects && i < app.effects.length; i += 1) {
            if (String(app.effects[i].matchName) === "ScatterMap") {
                report.found = true;
                report.matchName = String(app.effects[i].matchName);
                report.displayName = String(app.effects[i].displayName);
                break;
            }
        }
        writeMarker("scan complete found=" + report.found);
        if (report.found) {
            writeMarker("create comp");
            var comp = app.project.items.addComp("AEXCompatProbe", 4, 4, 1, 1 / 24, 24);
            var layer = comp.layers.addSolid([0, 0, 0], "Probe", 4, 4, 1, 1);
            writeMarker("add effect");
            var effect = layer.property("ADBE Effect Parade").addProperty("ScatterMap");
            report.added = effect !== null;
            report.propertyCount = effect ? effect.numProperties : 0;
            writeMarker("effect added properties=" + report.propertyCount);
        }
    } catch (err) {
        report.error = String(err && err.message ? err.message : err);
        writeMarker("probe error: " + report.error);
    } finally {
        try { writeReport(report); } catch (reportError) { writeMarker("report error: " + reportError.toString()); }
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false);
        app.quit();
    }
}());
