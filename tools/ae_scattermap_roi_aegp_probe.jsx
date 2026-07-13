(function () {
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
    marker("started " + app.version);
    app.beginSuppressDialogs();
    try {
        app.newProject(); app.project.bitsPerChannel = 8;
        try { app.project.workingSpace = "None"; } catch (colorError) {}
        var footage = app.project.importFile(new ImportOptions(new File(inputPath)));
        var comp = app.project.items.addComp("AEXCompatRoiAegpProbe", 16, 12, 1, 1 / 24, 24);
        var layer = comp.layers.add(footage);
        layer.property("ADBE Effect Parade").addProperty("ScatterMap");
        comp.openInViewer();
        var commandId = app.findMenuCommandId("AEXCompat ROI Probe");
        if (!commandId) {
            var markerFile = new File($.getenv("AEXCOMPAT_AE_MARKER"));
            if (markerFile.open("r")) {
                var markerText = markerFile.read(); markerFile.close();
                var match = /roi_aegp_command_id ([0-9]+)/.exec(markerText);
                if (match) { commandId = parseInt(match[1], 10); }
            }
        }
        if (!commandId) { throw new Error("AEXCompat ROI Probe command not registered"); }
        marker("execute roi command " + commandId);
        app.executeCommand(commandId);
        marker("roi command complete");
    } catch (error) {
        marker("probe error: " + String(error && error.message ? error.message : error));
    } finally {
        try { if (app.project) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } } catch (closeError) {}
        app.endSuppressDialogs(false); app.quit();
    }
}());
