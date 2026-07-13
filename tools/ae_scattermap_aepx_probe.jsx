(function () {
    function write(path, text) {
        var file = new File(path);
        if (!file.parent.exists) { file.parent.create(); }
        file.encoding = "UTF-8";
        if (!file.open("w")) { throw new Error("cannot create report"); }
        file.write(text); file.close();
    }
    var projectPath = $.getenv("AEXCOMPAT_AE_PROJECT");
    var reportPath = $.getenv("AEXCOMPAT_AE_REPORT");
    var errorText = "", saved = false;
    app.beginSuppressDialogs();
    try {
        app.newProject(); app.project.bitsPerChannel = 8;
        var comp = app.project.items.addComp("AEXCompatAepxProbe", 4, 4, 1, 1 / 24, 24);
        var layer = comp.layers.addSolid([0, 0, 0], "Probe", 4, 4, 1, 1);
        layer.property("ADBE Effect Parade").addProperty("ScatterMap");
        var projectFile = new File(projectPath);
        if (projectFile.exists) { throw new Error("project output already exists"); }
        saved = app.project.save(projectFile);
    } catch (error) {
        errorText = String(error && error.message ? error.message : error);
    } finally {
        write(reportPath, '{"schema_version":1,"app_version":"' + app.version +
            '","saved":' + saved + ',"error":"' + errorText.replace(/"/g, '\\"') + '"}\n');
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false); app.quit();
    }
}());
