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
    function attempt(property, propertyName, value) {
        var before = property.value, errorText = "";
        try { property.setValue(value); }
        catch (error) { errorText = String(error && error.message ? error.message : error); }
        return '{"property":' + esc(propertyName) + ',"attempted":' + value +
            ',"before":' + before + ',"after":' + property.value +
            ',"error":' + esc(errorText) + '}';
    }
    var reportPath = $.getenv("AEXCOMPAT_AE_REPORT");
    var reportFile = new File(reportPath);
    if (reportFile.exists) { throw new Error("report already exists"); }
    var results = [], errorText = "";
    marker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        app.newProject(); app.project.bitsPerChannel = 8;
        var comp = app.project.items.addComp("AEXCompatParamBoundsProbe", 16, 12, 1, 1 / 24, 24);
        var layer = comp.layers.addSolid([0, 0, 0], "Bounds Source", 16, 12, 1);
        var effect = layer.property("ADBE Effect Parade").addProperty("ScatterMap");
        var cases = [
            {name:"Scatter Amount", values:[-1, 501]},
            {name:"Direction", values:[0, 4]},
            {name:"Random Seed", values:[-1, 10001]},
            {name:"Mix with Original", values:[-0.1, 100.1]},
            {name:"Invert Map", values:[-1, 2]}
        ];
        for (var i = 0; i < cases.length; i += 1) {
            var property = effect.property(cases[i].name);
            if (!property) { throw new Error("property unavailable: " + cases[i].name); }
            for (var j = 0; j < cases[i].values.length; j += 1) {
                results.push(attempt(property, cases[i].name, cases[i].values[j]));
            }
        }
    } catch (error) {
        errorText = String(error && error.message ? error.message : error);
        marker("probe error: " + errorText);
    } finally {
        var json = '{"schema_version":1,"app_version":' + esc(app.version) +
            ',"attempts":[' + results.join(",") + '],"error":' + esc(errorText) + '}\n';
        try { write(reportPath, json, false); } catch (reportError) { marker("report error: " + reportError); }
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false); app.quit();
    }
}());
